use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};

use super::wordlist::WordlistData;
use crate::template::render::InputMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputCase {
    pub values: InputMap,
    pub display: String,
}

#[derive(Debug, Clone)]
pub struct InputCases {
    total: usize,
    generated: usize,
    kind: InputCaseKind,
}

#[derive(Debug, Clone)]
pub struct ScopeCases {
    total: usize,
    generated: usize,
    kind: ScopeCaseKind,
}

#[derive(Debug, Clone)]
enum InputCaseKind {
    Pitchfork {
        wordlists: Arc<Vec<WordlistData>>,
    },
    Clusterbomb {
        wordlists: Arc<Vec<WordlistData>>,
    },
    RotateWindow {
        wordlists: Arc<Vec<WordlistData>>,
        target_index: usize,
        target_window: usize,
        target_burst: usize,
        other_total: usize,
    },
}

#[derive(Debug, Clone)]
enum ScopeCaseKind {
    Single {
        case: Option<InputCase>,
    },
    Pitchfork {
        wordlists: Arc<Vec<WordlistData>>,
        next: usize,
        end: usize,
    },
    Clusterbomb {
        wordlists: Arc<Vec<WordlistData>>,
        next: usize,
        end: usize,
    },
}

impl InputCases {
    pub fn total(&self) -> usize {
        self.total
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    pub fn scope_is_prefix(&self, keywords: &[String]) -> bool {
        match &self.kind {
            InputCaseKind::Clusterbomb { wordlists } | InputCaseKind::Pitchfork { wordlists } => {
                scope_prefix_len(wordlists, keywords).is_some()
            }
            InputCaseKind::RotateWindow { .. } => false,
        }
    }

    pub fn next_scope(&mut self, keywords: &[String]) -> Option<ScopeCases> {
        if self.generated >= self.total {
            return None;
        }

        match &self.kind {
            InputCaseKind::Clusterbomb { wordlists } => {
                let Some(prefix_len) = scope_prefix_len(wordlists, keywords) else {
                    return self.next().map(ScopeCases::single);
                };
                let block_size = wordlists[prefix_len..]
                    .iter()
                    .map(|wordlist| wordlist.values.len())
                    .fold(1usize, usize::saturating_mul);
                let block_offset = self.generated % block_size;
                let end = self
                    .generated
                    .saturating_add(block_size.saturating_sub(block_offset))
                    .min(self.total);
                let scope = ScopeCases {
                    total: end - self.generated,
                    generated: 0,
                    kind: ScopeCaseKind::Clusterbomb {
                        wordlists: wordlists.clone(),
                        next: self.generated,
                        end,
                    },
                };
                self.generated = end;
                Some(scope)
            }
            InputCaseKind::Pitchfork { wordlists } => {
                let start = self.generated;
                let first = case_from_pitchfork_index(wordlists, start);
                let mut end = start;
                while end < self.total
                    && scope_matches(
                        &case_from_pitchfork_index(wordlists, end).values,
                        keywords,
                        &first.values,
                    )
                {
                    end += 1;
                }
                let scope = ScopeCases {
                    total: end - start,
                    generated: 0,
                    kind: ScopeCaseKind::Pitchfork {
                        wordlists: wordlists.clone(),
                        next: start,
                        end,
                    },
                };
                self.generated = end;
                Some(scope)
            }
            InputCaseKind::RotateWindow { .. } => self.next().map(ScopeCases::single),
        }
    }
}

impl Iterator for InputCases {
    type Item = InputCase;

    fn next(&mut self) -> Option<Self::Item> {
        if self.generated >= self.total {
            return None;
        }

        let case = match &self.kind {
            InputCaseKind::Pitchfork { wordlists } => {
                case_from_pitchfork_index(wordlists, self.generated)
            }
            InputCaseKind::Clusterbomb { wordlists } => {
                case_from_clusterbomb_offset(wordlists, self.generated)
            }
            InputCaseKind::RotateWindow {
                wordlists,
                target_index,
                target_window,
                target_burst,
                other_total,
            } => case_from_rotate_window_offset(
                wordlists,
                *target_index,
                *target_window,
                *target_burst,
                *other_total,
                self.generated,
            ),
        };
        self.generated += 1;
        Some(case)
    }
}

impl ScopeCases {
    pub fn len_remaining(&self) -> usize {
        self.total.saturating_sub(self.generated)
    }

    fn single(case: InputCase) -> Self {
        Self {
            total: 1,
            generated: 0,
            kind: ScopeCaseKind::Single { case: Some(case) },
        }
    }
}

impl Iterator for ScopeCases {
    type Item = InputCase;

    fn next(&mut self) -> Option<Self::Item> {
        if self.generated >= self.total {
            return None;
        }

        let case = match &mut self.kind {
            ScopeCaseKind::Single { case } => case.take()?,
            ScopeCaseKind::Pitchfork {
                wordlists,
                next,
                end,
            } => {
                if *next >= *end {
                    return None;
                }
                let case = case_from_pitchfork_index(wordlists, *next);
                *next += 1;
                case
            }
            ScopeCaseKind::Clusterbomb {
                wordlists,
                next,
                end,
            } => {
                if *next >= *end {
                    return None;
                }
                let case = case_from_clusterbomb_offset(wordlists, *next);
                *next += 1;
                case
            }
        };
        self.generated += 1;
        Some(case)
    }
}

pub fn pitchfork(wordlists: Vec<WordlistData>, budget: Option<usize>) -> InputCases {
    let limit = wordlists
        .iter()
        .map(|wordlist| wordlist.values.len())
        .min()
        .unwrap_or(0);
    let total = budget.map_or(limit, |budget| limit.min(budget));

    InputCases {
        total,
        generated: 0,
        kind: InputCaseKind::Pitchfork {
            wordlists: Arc::new(wordlists),
        },
    }
}

pub fn clusterbomb(wordlists: Vec<WordlistData>, budget: Option<usize>) -> Result<InputCases> {
    let raw_total = wordlists
        .iter()
        .map(|wordlist| wordlist.values.len())
        .fold(1usize, usize::saturating_mul);
    if raw_total > 1_000_000 {
        tracing::warn!(
            "clusterbomb will generate {} requests before filters",
            raw_total
        );
    }
    let total = budget.map_or(raw_total, |budget| raw_total.min(budget));

    Ok(InputCases {
        total,
        generated: 0,
        kind: InputCaseKind::Clusterbomb {
            wordlists: Arc::new(wordlists),
        },
    })
}

pub fn clusterbomb_ordered(
    wordlists: Vec<WordlistData>,
    order: &[String],
    budget: Option<usize>,
) -> Result<InputCases> {
    let mut ordered = Vec::with_capacity(wordlists.len());
    let mut remaining = wordlists;
    for keyword in order {
        let Some(position) = remaining
            .iter()
            .position(|wordlist| wordlist.keyword == *keyword)
        else {
            return Err(anyhow!("-order references unknown keyword: {}", keyword));
        };
        ordered.push(remaining.remove(position));
    }
    if !remaining.is_empty() {
        let missing = remaining
            .iter()
            .map(|wordlist| wordlist.keyword.as_str())
            .collect::<Vec<_>>()
            .join(",");
        return Err(anyhow!(
            "-order must include every wordlist keyword; missing: {missing}"
        ));
    }
    clusterbomb(ordered, budget)
}

pub fn clusterbomb_rotate_window(
    wordlists: Vec<WordlistData>,
    target_key: &str,
    target_window: usize,
    target_burst: usize,
    budget: Option<usize>,
) -> Result<InputCases> {
    let Some(target_index) = wordlists
        .iter()
        .position(|wordlist| wordlist.keyword == target_key)
    else {
        return Err(anyhow!(
            "-target-key references unknown keyword: {}",
            target_key
        ));
    };

    let raw_total = wordlists
        .iter()
        .map(|wordlist| wordlist.values.len())
        .fold(1usize, usize::saturating_mul);
    if raw_total > 1_000_000 {
        tracing::warn!(
            "clusterbomb will generate {} requests before filters",
            raw_total
        );
    }
    let target_len = wordlists[target_index].values.len();
    let other_total = raw_total.checked_div(target_len).unwrap_or(0);
    let total = budget.map_or(raw_total, |budget| raw_total.min(budget));

    Ok(InputCases {
        total,
        generated: 0,
        kind: InputCaseKind::RotateWindow {
            wordlists: Arc::new(wordlists),
            target_index,
            target_window: target_window.max(1),
            target_burst: target_burst.max(1),
            other_total,
        },
    })
}

fn case_from_pitchfork_index(wordlists: &[WordlistData], index: usize) -> InputCase {
    let mut values = BTreeMap::new();
    for wordlist in wordlists {
        values.insert(wordlist.keyword.clone(), wordlist.values[index].clone());
    }
    let display = display_input(&values);
    InputCase { values, display }
}

fn case_from_clusterbomb_offset(wordlists: &[WordlistData], offset: usize) -> InputCase {
    let mut remaining = offset;
    let mut indexes = vec![0; wordlists.len()];
    for position in (0..wordlists.len()).rev() {
        let len = wordlists[position].values.len();
        indexes[position] = remaining % len;
        remaining /= len;
    }

    let mut values = BTreeMap::new();
    for (wordlist, index) in wordlists.iter().zip(indexes) {
        values.insert(wordlist.keyword.clone(), wordlist.values[index].clone());
    }
    let display = display_input(&values);
    InputCase { values, display }
}

fn case_from_rotate_window_offset(
    wordlists: &[WordlistData],
    target_index: usize,
    target_window: usize,
    target_burst: usize,
    other_total: usize,
    offset: usize,
) -> InputCase {
    let target_len = wordlists[target_index].values.len();
    let window_size = target_window.min(target_len).max(1);
    let full_window_items = window_size.saturating_mul(other_total);
    let full_window_count = if full_window_items == 0 {
        0
    } else {
        target_len / window_size
    };
    let full_part_items = full_window_count.saturating_mul(full_window_items);

    let (target_window_start, target_window_len, within_window) = if offset < full_part_items {
        let window_number = offset / full_window_items;
        (
            window_number * window_size,
            window_size,
            offset % full_window_items,
        )
    } else {
        let remaining_targets = target_len.saturating_sub(full_window_count * window_size);
        (
            full_window_count * window_size,
            remaining_targets.max(1),
            offset.saturating_sub(full_part_items),
        )
    };

    let (target_position, other_offset) =
        rotate_window_positions(within_window, target_window_len, other_total, target_burst);
    let target_value_index = target_window_start + target_position;
    let mut indexes = indexes_from_other_offset(wordlists, target_index, other_offset);
    indexes[target_index] = target_value_index;

    let mut values = BTreeMap::new();
    for (wordlist, index) in wordlists.iter().zip(indexes) {
        values.insert(wordlist.keyword.clone(), wordlist.values[index].clone());
    }
    let display = display_input(&values);
    InputCase { values, display }
}

fn rotate_window_positions(
    within_window: usize,
    target_window_len: usize,
    other_total: usize,
    target_burst: usize,
) -> (usize, usize) {
    let full_burst_groups = other_total / target_burst;
    let remainder = other_total % target_burst;
    let full_group_items = target_window_len * target_burst;
    let full_part_items = full_burst_groups * full_group_items;

    if within_window < full_part_items {
        let group = within_window / full_group_items;
        let within_group = within_window % full_group_items;
        let target_position = within_group / target_burst;
        let burst_position = within_group % target_burst;
        return (target_position, group * target_burst + burst_position);
    }

    let remainder_len = remainder.max(1);
    let within_remainder = within_window.saturating_sub(full_part_items);
    let target_position = within_remainder / remainder_len;
    let burst_position = within_remainder % remainder_len;
    (
        target_position,
        full_burst_groups * target_burst + burst_position,
    )
}

fn indexes_from_other_offset(
    wordlists: &[WordlistData],
    target_index: usize,
    mut other_offset: usize,
) -> Vec<usize> {
    let mut indexes = vec![0; wordlists.len()];
    for position in (0..wordlists.len()).rev() {
        if position == target_index {
            continue;
        }
        let len = wordlists[position].values.len();
        indexes[position] = other_offset % len;
        other_offset /= len;
    }
    indexes
}

fn scope_prefix_len(wordlists: &[WordlistData], keywords: &[String]) -> Option<usize> {
    if keywords.len() > wordlists.len() {
        return None;
    }
    for (wordlist, keyword) in wordlists.iter().zip(keywords) {
        if wordlist.keyword != *keyword {
            return None;
        }
    }
    Some(keywords.len())
}

fn scope_matches(values: &InputMap, keywords: &[String], current_values: &InputMap) -> bool {
    keywords.iter().all(|keyword| {
        values
            .get(keyword)
            .zip(current_values.get(keyword))
            .is_some_and(|(left, right)| left == right)
    })
}

fn display_input(values: &InputMap) -> String {
    if values.len() == 1 {
        return values.values().next().cloned().unwrap_or_default();
    }
    values
        .iter()
        .map(|(key, value)| format!("{}={}", key, value))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wl(keyword: &str, values: &[&str]) -> WordlistData {
        WordlistData {
            keyword: keyword.to_string(),
            values: values.iter().map(|value| value.to_string()).collect(),
        }
    }

    #[test]
    fn pitchfork_streams_synchronized_rows() {
        let cases = pitchfork(
            vec![wl("USER", &["a", "b"]), wl("PASS", &["1", "2", "3"])],
            None,
        );
        assert_eq!(cases.total(), 2);

        let cases = cases.collect::<Vec<_>>();
        assert_eq!(cases.len(), 2);
        assert_eq!(cases[0].values["USER"], "a");
        assert_eq!(cases[0].values["PASS"], "1");
        assert_eq!(cases[1].values["USER"], "b");
        assert_eq!(cases[1].values["PASS"], "2");
    }

    #[test]
    fn clusterbomb_streams_cartesian_product_without_precollecting_cases() {
        let cases = clusterbomb(vec![wl("A", &["1", "2"]), wl("B", &["x", "y"])], None).unwrap();
        assert_eq!(cases.total(), 4);

        let rendered = cases
            .map(|case| format!("{}{}", case.values["A"], case.values["B"]))
            .collect::<Vec<_>>();
        assert_eq!(rendered, vec!["1x", "1y", "2x", "2y"]);
    }

    #[test]
    fn clusterbomb_respects_budget_without_generating_full_product() {
        let cases = clusterbomb(
            vec![wl("A", &["1", "2", "3"]), wl("B", &["x", "y", "z"])],
            Some(2),
        )
        .unwrap();

        assert_eq!(cases.total(), 2);
        assert_eq!(cases.count(), 2);
    }

    #[test]
    fn clusterbomb_splits_prefix_scopes() {
        let mut cases = clusterbomb(
            vec![
                wl("URLFUZZ", &["u1", "u2"]),
                wl("UFUZZ", &["alice", "bob"]),
                wl("PFUZZ", &["p1", "p2", "p3"]),
            ],
            None,
        )
        .unwrap();

        let first_scope = cases
            .next_scope(&["URLFUZZ".to_string(), "UFUZZ".to_string()])
            .unwrap()
            .collect::<Vec<_>>();
        let second_scope = cases
            .next_scope(&["URLFUZZ".to_string(), "UFUZZ".to_string()])
            .unwrap()
            .collect::<Vec<_>>();

        assert_eq!(first_scope.len(), 3);
        assert!(first_scope
            .iter()
            .all(|case| case.values["URLFUZZ"] == "u1"));
        assert!(first_scope
            .iter()
            .all(|case| case.values["UFUZZ"] == "alice"));
        assert_eq!(second_scope.len(), 3);
        assert!(second_scope
            .iter()
            .all(|case| case.values["URLFUZZ"] == "u1"));
        assert!(second_scope
            .iter()
            .all(|case| case.values["UFUZZ"] == "bob"));
    }

    #[test]
    fn clusterbomb_respects_custom_keyword_order() {
        let cases = clusterbomb_ordered(
            vec![
                wl("URLFUZZ", &["url1", "url2"]),
                wl("UFUZZ", &["alice", "bob"]),
                wl("PFUZZ", &["p1", "p2"]),
            ],
            &[
                "UFUZZ".to_string(),
                "PFUZZ".to_string(),
                "URLFUZZ".to_string(),
            ],
            None,
        )
        .unwrap();

        let rendered = cases
            .map(|case| {
                format!(
                    "{}:{}:{}",
                    case.values["UFUZZ"], case.values["PFUZZ"], case.values["URLFUZZ"]
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            rendered,
            vec![
                "alice:p1:url1",
                "alice:p1:url2",
                "alice:p2:url1",
                "alice:p2:url2",
                "bob:p1:url1",
                "bob:p1:url2",
                "bob:p2:url1",
                "bob:p2:url2",
            ]
        );
    }

    #[test]
    fn ordered_clusterbomb_splits_batches_by_all_but_fastest_keyword() {
        let mut cases = clusterbomb_ordered(
            vec![
                wl("URLFUZZ", &["url1", "url2"]),
                wl("UFUZZ", &["alice", "bob"]),
                wl("PFUZZ", &["p1", "p2"]),
            ],
            &[
                "UFUZZ".to_string(),
                "PFUZZ".to_string(),
                "URLFUZZ".to_string(),
            ],
            None,
        )
        .unwrap();

        let first_batch = cases
            .next_scope(&["UFUZZ".to_string(), "PFUZZ".to_string()])
            .unwrap()
            .collect::<Vec<_>>();
        let second_batch = cases
            .next_scope(&["UFUZZ".to_string(), "PFUZZ".to_string()])
            .unwrap()
            .collect::<Vec<_>>();

        assert_eq!(first_batch.len(), 2);
        assert!(first_batch
            .iter()
            .all(|case| case.values["UFUZZ"] == "alice"));
        assert!(first_batch.iter().all(|case| case.values["PFUZZ"] == "p1"));
        assert_eq!(first_batch[0].values["URLFUZZ"], "url1");
        assert_eq!(first_batch[1].values["URLFUZZ"], "url2");

        assert_eq!(second_batch.len(), 2);
        assert!(second_batch
            .iter()
            .all(|case| case.values["UFUZZ"] == "alice"));
        assert!(second_batch.iter().all(|case| case.values["PFUZZ"] == "p2"));
    }

    #[test]
    fn rotate_window_keeps_targets_hot_in_small_windows() {
        let cases = clusterbomb_rotate_window(
            vec![
                wl("URLFUZZ", &["u1", "u2", "u3"]),
                wl("UFUZZ", &["alice", "bob"]),
                wl("PFUZZ", &["p1", "p2", "p3"]),
            ],
            "URLFUZZ",
            2,
            2,
            None,
        )
        .unwrap();

        assert_eq!(cases.total(), 18);

        let rendered = cases
            .take(12)
            .map(|case| {
                format!(
                    "{}:{}:{}",
                    case.values["URLFUZZ"], case.values["UFUZZ"], case.values["PFUZZ"]
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            rendered,
            vec![
                "u1:alice:p1",
                "u1:alice:p2",
                "u2:alice:p1",
                "u2:alice:p2",
                "u1:alice:p3",
                "u1:bob:p1",
                "u2:alice:p3",
                "u2:bob:p1",
                "u1:bob:p2",
                "u1:bob:p3",
                "u2:bob:p2",
                "u2:bob:p3",
            ]
        );
    }

    #[test]
    fn rotate_window_marks_scope_as_non_prefix_for_global_stop_tracking() {
        let cases = clusterbomb_rotate_window(
            vec![
                wl("URLFUZZ", &["u1", "u2"]),
                wl("UFUZZ", &["alice"]),
                wl("PFUZZ", &["p1", "p2"]),
            ],
            "URLFUZZ",
            2,
            1,
            None,
        )
        .unwrap();

        assert!(!cases.scope_is_prefix(&["URLFUZZ".to_string()]));
    }
}
