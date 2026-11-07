// File: src/app/breadcrumbs.rs
// Purpose: Interactive single-line breadcrumb path bar model and width layout calculation.

use std::path::PathBuf;
use crate::filesystem::truncate;

/// A segment in the interactive breadcrumbs path bar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BreadcrumbSegment {
    /// A single navigable folder button.
    Crumb {
        label: String,
        path: PathBuf,
        is_current: bool,
    },
    /// Intermediate folders collapsed into a button ("...") to fit within available width.
    Ellipsis {
        hidden: Vec<(String, PathBuf)>,
    },
}

/// Compute the breadcrumb segments to render inside the available address bar width.
/// When the path exceeds the width, intermediate folders collapse into an Ellipsis segment ("..."),
/// preserving the root (drive) and the active folder as interactive buttons.
pub(crate) fn layout_breadcrumbs(
    crumbs: &[(String, PathBuf)],
    available_width: f32,
    measure_text_width: impl Fn(&str, bool) -> f32,
) -> Vec<BreadcrumbSegment> {
    if crumbs.is_empty() {
        return Vec::new();
    }

    let n = crumbs.len();
    let btn_pad = 8.0_f32; // 4.0 left + 4.0 right button padding
    let sep_w = measure_text_width("›", false) + 4.0;
    let ellipsis_w = measure_text_width("...", false) + btn_pad;

    let crumb_widths: Vec<f32> = crumbs
        .iter()
        .enumerate()
        .map(|(i, (label, _))| {
            let is_last = i == n - 1;
            measure_text_width(label, is_last) + btn_pad
        })
        .collect();

    let total_uncollapsed: f32 = crumb_widths.iter().sum::<f32>()
        + if n > 1 { (n - 1) as f32 * sep_w } else { 0.0 };

    if total_uncollapsed <= available_width || n <= 2 {
        if total_uncollapsed <= available_width || n <= 1 {
            return crumbs
                .iter()
                .enumerate()
                .map(|(i, (label, path))| BreadcrumbSegment::Crumb {
                    label: label.clone(),
                    path: path.clone(),
                    is_current: i == n - 1,
                })
                .collect();
        }
    }

    // Try keeping crumbs[0] (root), an Ellipsis for intermediate crumbs (1..k),
    // and as many trailing crumbs (k..n) as possible, always including crumbs[n-1].
    let mut best_k = None;
    for k in 2..n {
        let trailing_w: f32 = crumb_widths[k..n].iter().sum::<f32>()
            + (n - k) as f32 * sep_w;
        let candidate_w = crumb_widths[0] + sep_w + ellipsis_w + trailing_w;
        if candidate_w <= available_width {
            best_k = Some(k);
            break;
        }
    }

    if let Some(k) = best_k {
        let mut segments = Vec::new();
        segments.push(BreadcrumbSegment::Crumb {
            label: crumbs[0].0.clone(),
            path: crumbs[0].1.clone(),
            is_current: false,
        });
        segments.push(BreadcrumbSegment::Ellipsis {
            hidden: crumbs[1..k].to_vec(),
        });
        for i in k..n {
            segments.push(BreadcrumbSegment::Crumb {
                label: crumbs[i].0.clone(),
                path: crumbs[i].1.clone(),
                is_current: i == n - 1,
            });
        }
        return segments;
    }

    // If even root + ... + current does not fit, collapse root as well: ... + current
    let last_w = crumb_widths[n - 1];
    let candidate_no_root = ellipsis_w + sep_w + last_w;
    if candidate_no_root <= available_width {
        return vec![
            BreadcrumbSegment::Ellipsis {
                hidden: crumbs[0..n - 1].to_vec(),
            },
            BreadcrumbSegment::Crumb {
                label: crumbs[n - 1].0.clone(),
                path: crumbs[n - 1].1.clone(),
                is_current: true,
            },
        ];
    }

    // If even that exceeds available width, truncate the current folder's label
    let avail_for_last = (available_width - ellipsis_w - sep_w).max(10.0);
    let original_label = &crumbs[n - 1].0;
    let avg_char_w = (last_w / original_label.chars().count().max(1) as f32).max(6.0);
    let max_chars = ((avail_for_last - btn_pad) / avg_char_w).floor().max(3.0) as usize;
    let truncated_label = truncate(original_label, max_chars);

    vec![
        BreadcrumbSegment::Ellipsis {
            hidden: crumbs[0..n - 1].to_vec(),
        },
        BreadcrumbSegment::Crumb {
            label: truncated_label,
            path: crumbs[n - 1].1.clone(),
            is_current: true,
        },
    ]
}
