//! Completion for lints
use ide_db::helpers::generated_lints::Lint;
use syntax::{ast, T};

use crate::{
    context::CompletionContext,
    item::{CompletionItem, CompletionItemKind},
    Completions,
};

pub(super) fn complete_lint(
    acc: &mut Completions,
    ctx: &CompletionContext,
    existing_lints: &[ast::Path],
    lints_completions: &[Lint],
) {
    let is_qualified = ctx.previous_token_is(T![:]);
    for &Lint { label, description } in lints_completions {
        let (qual, name) = {
            // FIXME: change `Lint`'s label to not store a path in it but split the prefix off instead?
            let mut parts = label.split("::");
            let ns_or_label = match parts.next() {
                Some(it) => it,
                None => continue,
            };
            let label = parts.next();
            match label {
                Some(label) => (Some(ns_or_label), label),
                None => (None, ns_or_label),
            }
        };
        if qual.is_none() && is_qualified {
            // qualified completion requested, but this lint is unqualified
            continue;
        }
        let lint_already_annotated = existing_lints
            .iter()
            .filter_map(|path| {
                let q = path.qualifier();
                if q.as_ref().and_then(|it| it.qualifier()).is_some() {
                    return None;
                }
                Some((q.and_then(|it| it.as_single_name_ref()), path.segment()?.name_ref()?))
            })
            .any(|(q, name_ref)| {
                let qualifier_matches = match (q, qual) {
                    (None, None) => true,
                    (None, Some(_)) => false,
                    (Some(_), None) => false,
                    (Some(q), Some(ns)) => q.text() == ns,
                };
                qualifier_matches && name_ref.text() == name
            });
        if lint_already_annotated {
            continue;
        }
        let label = match qual {
            Some(qual) if !is_qualified => format!("{}::{}", qual, name),
            _ => name.to_owned(),
        };
        let mut item =
            CompletionItem::new(CompletionItemKind::Attribute, ctx.source_range(), label);
        item.documentation(hir::Documentation::new(description.to_owned()));
        item.add_to(acc)
    }
}
