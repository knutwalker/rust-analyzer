//! This crate provides primitives for tracking the information about a call site.
use base_db::FilePosition;
use either::Either;
use hir::{HasAttrs, HirDisplay, Semantics, Type};
use stdx::format_to;
use syntax::{
    algo,
    ast::{self, HasArgList, HasName},
    AstNode, Direction, SyntaxToken, TextRange, TextSize,
};

use crate::RootDatabase;

/// Contains information about a call site. Specifically the
/// `FunctionSignature`and current parameter.
#[derive(Debug)]
pub struct CallInfo {
    pub doc: Option<String>,
    pub signature: String,
    pub active_parameter: Option<usize>,
    parameters: Vec<TextRange>,
}

impl CallInfo {
    pub fn parameter_labels(&self) -> impl Iterator<Item = &str> + '_ {
        self.parameters.iter().map(move |&it| &self.signature[it])
    }

    pub fn parameter_ranges(&self) -> &[TextRange] {
        &self.parameters
    }

    fn push_param(&mut self, param: &str) {
        if !self.signature.ends_with('(') {
            self.signature.push_str(", ");
        }
        let start = TextSize::of(&self.signature);
        self.signature.push_str(param);
        let end = TextSize::of(&self.signature);
        self.parameters.push(TextRange::new(start, end))
    }
}

/// Computes parameter information for the given call expression.
pub fn call_info(db: &RootDatabase, position: FilePosition) -> Option<CallInfo> {
    let sema = Semantics::new(db);
    let file = sema.parse(position.file_id);
    let file = file.syntax();
    let token = file
        .token_at_offset(position.offset)
        .left_biased()
        // if the cursor is sandwiched between two space tokens and the call is unclosed
        // this prevents us from leaving the CallExpression
        .and_then(|tok| algo::skip_trivia_token(tok, Direction::Prev))?;
    let token = sema.descend_into_macros_single(token);

    let (callable, active_parameter) = call_info_impl(&sema, token)?;

    let mut res =
        CallInfo { doc: None, signature: String::new(), parameters: vec![], active_parameter };

    match callable.kind() {
        hir::CallableKind::Function(func) => {
            res.doc = func.docs(db).map(|it| it.into());
            format_to!(res.signature, "fn {}", func.name(db));
        }
        hir::CallableKind::TupleStruct(strukt) => {
            res.doc = strukt.docs(db).map(|it| it.into());
            format_to!(res.signature, "struct {}", strukt.name(db));
        }
        hir::CallableKind::TupleEnumVariant(variant) => {
            res.doc = variant.docs(db).map(|it| it.into());
            format_to!(
                res.signature,
                "enum {}::{}",
                variant.parent_enum(db).name(db),
                variant.name(db)
            );
        }
        hir::CallableKind::Closure => (),
    }

    res.signature.push('(');
    {
        if let Some(self_param) = callable.receiver_param(db) {
            format_to!(res.signature, "{}", self_param)
        }
        let mut buf = String::new();
        for (pat, ty) in callable.params(db) {
            buf.clear();
            if let Some(pat) = pat {
                match pat {
                    Either::Left(_self) => format_to!(buf, "self: "),
                    Either::Right(pat) => format_to!(buf, "{}: ", pat),
                }
            }
            format_to!(buf, "{}", ty.display(db));
            res.push_param(&buf);
        }
    }
    res.signature.push(')');

    match callable.kind() {
        hir::CallableKind::Function(_) | hir::CallableKind::Closure => {
            let ret_type = callable.return_type();
            if !ret_type.is_unit() {
                format_to!(res.signature, " -> {}", ret_type.display(db));
            }
        }
        hir::CallableKind::TupleStruct(_) | hir::CallableKind::TupleEnumVariant(_) => {}
    }
    Some(res)
}

fn call_info_impl(
    sema: &Semantics<RootDatabase>,
    token: SyntaxToken,
) -> Option<(hir::Callable, Option<usize>)> {
    // Find the calling expression and it's NameRef
    let parent = token.parent()?;
    let calling_node = parent.ancestors().filter_map(ast::CallableExpr::cast).find(|it| {
        it.arg_list()
            .map_or(false, |it| it.syntax().text_range().contains(token.text_range().start()))
    })?;

    let callable = match &calling_node {
        ast::CallableExpr::Call(call) => {
            let expr = call.expr()?;
            sema.type_of_expr(&expr)?.adjusted().as_callable(sema.db)
        }
        ast::CallableExpr::MethodCall(call) => sema.resolve_method_call_as_callable(call),
    }?;
    let active_param = if let Some(arg_list) = calling_node.arg_list() {
        let param = arg_list
            .args()
            .take_while(|arg| arg.syntax().text_range().end() <= token.text_range().start())
            .count();
        Some(param)
    } else {
        None
    };
    Some((callable, active_param))
}

#[derive(Debug)]
pub struct ActiveParameter {
    pub ty: Type,
    pub pat: Either<ast::SelfParam, ast::Pat>,
}

impl ActiveParameter {
    pub fn at_token(sema: &Semantics<RootDatabase>, token: SyntaxToken) -> Option<Self> {
        let (signature, active_parameter) = call_info_impl(sema, token)?;

        let idx = active_parameter?;
        let mut params = signature.params(sema.db);
        if !(idx < params.len()) {
            cov_mark::hit!(too_many_arguments);
            return None;
        }
        let (pat, ty) = params.swap_remove(idx);
        pat.map(|pat| ActiveParameter { ty, pat })
    }

    pub fn ident(&self) -> Option<ast::Name> {
        self.pat.as_ref().right().and_then(|param| match param {
            ast::Pat::IdentPat(ident) => ident.name(),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests;
