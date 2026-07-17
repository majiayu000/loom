use std::collections::{BTreeMap, BTreeSet};

use super::super::super::CommandFailure;
use super::{ScanPlan, SourcePlan, checked_field_add, checked_increment};

pub(super) fn coalesce_sources(plan: &mut ScanPlan) -> std::result::Result<(), CommandFailure> {
    let mut grouped = BTreeMap::<String, SourcePlan>::new();
    for source in std::mem::take(&mut plan.sources) {
        let key = source.source_key.clone();
        if let Some(existing) = grouped.get_mut(&key) {
            debug_assert_eq!(existing.agent, source.agent);
            debug_assert_eq!(existing.expected, source.expected);
            existing.source_guards.extend(source.source_guards);
            existing.drafts.extend(source.drafts);
            let source_continuous = source.expected.is_some() && source.reset_reason.is_none();
            let existing_continuous =
                existing.expected.is_some() && existing.reset_reason.is_none();
            if (source_continuous, &source.authority) > (existing_continuous, &existing.authority) {
                existing.checkpoint = source.checkpoint;
                existing.authority = source.authority;
                existing.reset_reason = source.reset_reason;
                existing.stats = source.stats;
                existing.rejected_reasons = source.rejected_reasons;
            }
        } else {
            grouped.insert(key, source);
        }
    }
    for source in grouped.values_mut() {
        let mut event_ids = BTreeSet::new();
        source.drafts.retain(|draft| {
            draft
                .event_id_override
                .as_ref()
                .is_none_or(|event_id| event_ids.insert(event_id.clone()))
        });
        if let Some(reason) = source.reset_reason {
            checked_increment(&mut plan.reset_reasons, reason.as_str().to_string())?;
        }
        let stats = plan.stats.entry(source.agent).or_default();
        checked_field_add(
            &mut stats.scanned_events,
            source.stats.scanned_events,
            "scanned_events",
        )?;
        checked_field_add(
            &mut stats.window_skipped,
            source.stats.window_skipped,
            "window_skipped",
        )?;
        checked_field_add(&mut stats.malformed, source.stats.malformed, "malformed")?;
        checked_field_add(
            &mut stats.pending_partial,
            source.stats.pending_partial,
            "pending_partial",
        )?;
        checked_field_add(&mut stats.rejected, source.stats.rejected, "rejected")?;
        for (reason, count) in &source.rejected_reasons {
            checked_field_add(
                plan.rejected_reasons.entry(reason.clone()).or_default(),
                *count,
                "rejected",
            )?;
        }
        for draft in &source.drafts {
            if let Some(name) = draft.observed_skill_name.as_ref() {
                checked_increment(
                    &mut plan.unmatched,
                    (name.clone(), source.agent.as_str().to_string()),
                )?;
            }
        }
    }
    plan.sources = grouped.into_values().collect();
    Ok(())
}
