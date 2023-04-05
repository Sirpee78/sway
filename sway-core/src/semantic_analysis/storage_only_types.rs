use sway_error::error::CompileError;
use sway_error::warning::CompileWarning;
use sway_types::{Span, Spanned};

use crate::{
    decl_engine::DeclId,
    engine_threading::*,
    error::*,
    language::ty::{self, TyConstantDecl, TyFunctionDecl},
    type_system::*,
};

fn ast_node_validate(engines: Engines<'_>, x: &ty::TyAstNodeContent) -> CompileResult<()> {
    let errors: Vec<CompileError> = vec![];
    let warnings: Vec<CompileWarning> = vec![];
    match x {
        ty::TyAstNodeContent::Expression(expr)
        | ty::TyAstNodeContent::ImplicitReturnExpression(expr) => expr_validate(engines, expr),
        ty::TyAstNodeContent::Declaration(decl) => decl_validate(engines, decl),
        ty::TyAstNodeContent::SideEffect(_) => ok((), warnings, errors),
    }
}

fn expr_validate(engines: Engines<'_>, expr: &ty::TyExpression) -> CompileResult<()> {
    let mut errors: Vec<CompileError> = vec![];
    let mut warnings: Vec<CompileWarning> = vec![];
    match &expr.expression {
        ty::TyExpressionVariant::Literal(_)
        | ty::TyExpressionVariant::VariableExpression { .. }
        | ty::TyExpressionVariant::FunctionParameter
        | ty::TyExpressionVariant::AsmExpression { .. }
        | ty::TyExpressionVariant::StorageAccess(_)
        | ty::TyExpressionVariant::AbiName(_) => (),
        ty::TyExpressionVariant::FunctionApplication { arguments, .. } => {
            for f in arguments {
                check!(expr_validate(engines, &f.1), continue, warnings, errors);
            }
        }
        ty::TyExpressionVariant::LazyOperator {
            lhs: expr1,
            rhs: expr2,
            ..
        }
        | ty::TyExpressionVariant::ArrayIndex {
            prefix: expr1,
            index: expr2,
        } => {
            check!(expr_validate(engines, expr1), (), warnings, errors);
            check!(expr_validate(engines, expr2), (), warnings, errors);
        }
        ty::TyExpressionVariant::IntrinsicFunction(ty::TyIntrinsicFunctionKind {
            arguments: exprvec,
            ..
        })
        | ty::TyExpressionVariant::Tuple { fields: exprvec }
        | ty::TyExpressionVariant::Array {
            elem_type: _,
            contents: exprvec,
        } => {
            for f in exprvec {
                check!(expr_validate(engines, f), continue, warnings, errors)
            }
        }
        ty::TyExpressionVariant::StructExpression { fields, .. } => {
            for f in fields {
                check!(expr_validate(engines, &f.value), continue, warnings, errors);
            }
        }
        ty::TyExpressionVariant::CodeBlock(cb) => {
            check!(
                validate_decls_for_storage_only_types_in_codeblock(engines, cb),
                (),
                warnings,
                errors
            );
        }
        ty::TyExpressionVariant::MatchExp { desugared, .. } => {
            check!(expr_validate(engines, desugared), (), warnings, errors)
        }
        ty::TyExpressionVariant::IfExp {
            condition,
            then,
            r#else,
        } => {
            check!(expr_validate(engines, condition), (), warnings, errors);
            check!(expr_validate(engines, then), (), warnings, errors);
            if let Some(r#else) = r#else {
                check!(expr_validate(engines, r#else), (), warnings, errors);
            }
        }
        ty::TyExpressionVariant::StructFieldAccess { prefix: exp, .. }
        | ty::TyExpressionVariant::TupleElemAccess { prefix: exp, .. }
        | ty::TyExpressionVariant::AbiCast { address: exp, .. }
        | ty::TyExpressionVariant::EnumTag { exp }
        | ty::TyExpressionVariant::UnsafeDowncast { exp, .. } => {
            check!(expr_validate(engines, exp), (), warnings, errors)
        }
        ty::TyExpressionVariant::EnumInstantiation { contents, .. } => {
            if let Some(f) = contents {
                check!(expr_validate(engines, f), (), warnings, errors);
            }
        }
        ty::TyExpressionVariant::WhileLoop { condition, body } => {
            check!(expr_validate(engines, condition), (), warnings, errors);
            check!(
                validate_decls_for_storage_only_types_in_codeblock(engines, body),
                (),
                warnings,
                errors
            );
        }
        ty::TyExpressionVariant::Break => (),
        ty::TyExpressionVariant::Continue => (),
        ty::TyExpressionVariant::Reassignment(reassignment) => {
            let ty::TyReassignment {
                lhs_base_name, rhs, ..
            } = &**reassignment;
            check!(
                check_type(engines, rhs.return_type, lhs_base_name.span(), false),
                (),
                warnings,
                errors,
            );
            check!(expr_validate(engines, rhs), (), warnings, errors)
        }
        ty::TyExpressionVariant::StorageReassignment(storage_reassignment) => {
            let span = storage_reassignment.span();
            let rhs = &storage_reassignment.rhs;
            check!(
                check_type(engines, rhs.return_type, span, false),
                (),
                warnings,
                errors,
            );
            check!(expr_validate(engines, rhs), (), warnings, errors)
        }
        ty::TyExpressionVariant::Return(exp) => {
            check!(expr_validate(engines, exp), (), warnings, errors)
        }
    }
    ok((), warnings, errors)
}

fn check_type(
    engines: Engines<'_>,
    ty: TypeId,
    span: Span,
    ignore_self: bool,
) -> CompileResult<()> {
    let mut warnings: Vec<CompileWarning> = vec![];
    let mut errors: Vec<CompileError> = vec![];

    let ty_engine = engines.te();
    let decl_engine = engines.de();

    let type_info = check!(
        CompileResult::from(ty_engine.to_typeinfo(ty, &span).map_err(CompileError::from)),
        TypeInfo::ErrorRecovery,
        warnings,
        errors
    );
    let nested_types = type_info.clone().extract_nested_types(engines);
    for ty in nested_types {
        if ignore_self && ty.eq(&type_info, engines) {
            continue;
        }
        if ty_engine.is_type_info_storage_only(decl_engine, &ty) {
            errors.push(CompileError::InvalidStorageOnlyTypeDecl {
                ty: engines.help_out(ty).to_string(),
                span: span.clone(),
            });
        }
    }
    ok((), warnings, errors)
}

fn decl_validate(engines: Engines<'_>, decl: &ty::TyDecl) -> CompileResult<()> {
    let mut warnings: Vec<CompileWarning> = vec![];
    let mut errors: Vec<CompileError> = vec![];
    let decl_engine = engines.de();
    match decl {
        ty::TyDecl::VariableDecl(decl) => {
            check!(
                check_type(engines, decl.body.return_type, decl.name.span(), false),
                (),
                warnings,
                errors
            );
            check!(expr_validate(engines, &decl.body), (), warnings, errors)
        }
        ty::TyDecl::ConstantDecl { decl_id, .. } => {
            check!(
                validate_const_decl(engines, decl_id),
                return err(warnings, errors),
                warnings,
                errors
            );
        }
        ty::TyDecl::FunctionDecl { decl_id, .. } => {
            check!(
                validate_fn_decl(engines, decl_id),
                return err(warnings, errors),
                warnings,
                errors
            );
        }
        ty::TyDecl::AbiDecl { .. } | ty::TyDecl::TraitDecl { .. } => {
            // These methods are not typed. They are however handled from ImplTrait.
        }
        ty::TyDecl::ImplTrait { decl_id, .. } => {
            let ty::TyImplTrait { items, .. } = decl_engine.get_impl_trait(decl_id);
            for item in items {
                match item {
                    ty::TyImplItem::Fn(decl_ref) => {
                        check!(
                            validate_fn_decl(engines, &decl_ref.id().clone()),
                            (),
                            warnings,
                            errors
                        );
                    }
                    ty::TyImplItem::Constant(decl_ref) => {
                        check!(
                            validate_const_decl(engines, decl_ref.id()),
                            return err(warnings, errors),
                            warnings,
                            errors
                        );
                    }
                }
            }
        }
        ty::TyDecl::StructDecl { decl_id, .. } => {
            let ty::TyStructDecl { fields, .. } = decl_engine.get_struct(decl_id);
            for field in fields {
                check!(
                    check_type(
                        engines,
                        field.type_argument.type_id,
                        field.span.clone(),
                        false
                    ),
                    continue,
                    warnings,
                    errors
                );
            }
        }
        ty::TyDecl::EnumDecl { decl_id, .. } => {
            let ty::TyEnumDecl { variants, .. } = decl_engine.get_enum(decl_id);
            for variant in variants {
                check!(
                    check_type(
                        engines,
                        variant.type_argument.type_id,
                        variant.span.clone(),
                        false
                    ),
                    continue,
                    warnings,
                    errors
                );
            }
        }
        ty::TyDecl::EnumVariantDecl {
            decl_id,
            variant_name,
            ..
        } => {
            let enum_decl = decl_engine.get_enum(decl_id);
            let variant = check!(
                enum_decl.expect_variant_from_name(variant_name),
                return err(warnings, errors),
                warnings,
                errors
            );
            check!(
                check_type(
                    engines,
                    variant.type_argument.type_id,
                    variant.span.clone(),
                    false
                ),
                (),
                warnings,
                errors
            );
        }
        ty::TyDecl::StorageDecl {
            decl_id,
            decl_span: _,
        } => {
            let ty::TyStorageDecl { fields, .. } = decl_engine.get_storage(decl_id);
            for field in fields {
                check!(
                    check_type(
                        engines,
                        field.type_argument.type_id,
                        field.name.span().clone(),
                        true
                    ),
                    continue,
                    warnings,
                    errors
                );
            }
        }
        ty::TyDecl::TypeAliasDecl { decl_id, .. } => {
            let ty::TyTypeAliasDecl { ty, span, .. } = decl_engine.get_type_alias(decl_id);
            check!(
                check_type(engines, ty.type_id, span, false),
                (),
                warnings,
                errors
            );
        }
        ty::TyDecl::GenericTypeForFunctionScope { .. } | ty::TyDecl::ErrorRecovery(_) => {}
    }
    if errors.is_empty() {
        ok((), warnings, errors)
    } else {
        err(warnings, errors)
    }
}

pub fn validate_const_decl(
    engines: Engines<'_>,
    decl_id: &DeclId<TyConstantDecl>,
) -> CompileResult<()> {
    let mut warnings: Vec<CompileWarning> = vec![];
    let mut errors: Vec<CompileError> = vec![];
    let decl_engine = engines.de();
    let ty::TyConstantDecl {
        value: expr,
        call_path,
        ..
    } = decl_engine.get_constant(decl_id);
    if let Some(expr) = expr {
        check!(
            check_type(engines, expr.return_type, call_path.suffix.span(), false),
            (),
            warnings,
            errors
        );
        check!(expr_validate(engines, &expr), (), warnings, errors)
    }
    if errors.is_empty() {
        ok((), warnings, errors)
    } else {
        err(warnings, errors)
    }
}

pub fn validate_fn_decl(
    engines: Engines<'_>,
    decl_id: &DeclId<TyFunctionDecl>,
) -> CompileResult<()> {
    let mut warnings: Vec<CompileWarning> = vec![];
    let mut errors: Vec<CompileError> = vec![];
    let decl_engine = engines.de();
    let ty::TyFunctionDecl {
        body,
        parameters,
        return_type,
        ..
    } = decl_engine.get_function(decl_id);
    check!(
        validate_decls_for_storage_only_types_in_codeblock(engines, &body),
        (),
        warnings,
        errors
    );
    for param in parameters {
        if !param.is_self() {
            check!(
                check_type(
                    engines,
                    param.type_argument.type_id,
                    param.type_argument.span.clone(),
                    false
                ),
                continue,
                warnings,
                errors
            );
        }
    }
    check!(
        check_type(engines, return_type.type_id, return_type.span, false),
        return err(warnings, errors),
        warnings,
        errors
    );
    ok((), warnings, errors)
}

pub fn validate_decls_for_storage_only_types_in_ast(
    engines: Engines<'_>,
    ast_n: &ty::TyAstNodeContent,
) -> CompileResult<()> {
    ast_node_validate(engines, ast_n)
}

pub fn validate_decls_for_storage_only_types_in_codeblock(
    engines: Engines<'_>,
    cb: &ty::TyCodeBlock,
) -> CompileResult<()> {
    let mut warnings: Vec<CompileWarning> = vec![];
    let mut errors: Vec<CompileError> = vec![];
    for x in &cb.contents {
        check!(
            ast_node_validate(engines, &x.content),
            continue,
            warnings,
            errors
        )
    }
    ok((), warnings, errors)
}