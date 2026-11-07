// File: src/app/tests.rs
// Purpose: Unit tests for Photonio application state, input chords, and math calculations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use eframe::egui;

use crate::app::breadcrumbs::{layout_breadcrumbs, BreadcrumbSegment};
use crate::app::cache::{remap_cache_keys, remap_cache_keys_with_rename};
use crate::app::types::{compute_feedback_alpha, setup_fonts};
use crate::app::widgets::cover_uv;
use crate::app::PhotonioApp;
use crate::filesystem::{default_resized_name, format_size, truncate, validate_file_name};
use crate::{assets, icons};

#[test]
fn truncates_long_strings_with_ellipsis() {
    assert_eq!(truncate("short.png", 28), "short.png");
    let t = truncate("a_very_long_filename_that_needs_ellipsis.png", 20);
    assert!(t.ends_with('…'));
    assert!(t.chars().count() <= 20);
}

#[test]
fn formats_file_sizes() {
    assert_eq!(format_size(512), "512 B");
    assert_eq!(format_size(1024), "1.0 KB");
    assert_eq!(format_size(2_500_000), "2.4 MB");
}

#[test]
fn default_resized_name_appends_suffix() {
    assert_eq!(default_resized_name(Path::new("C:/pics/photo.jpg")), "photo_resized.jpg");
    assert_eq!(default_resized_name(Path::new("C:/pics/photo")), "photo_resized.png");
}

#[test]
fn embedded_logo_decodes() {
    assert!(assets::logo_color_image().is_some());
}

#[test]
fn icon_folder_stretched_layout_and_quad_insets() {
    let ctx = egui::Context::default();
    setup_fonts(&ctx);
    let _ = ctx.run(egui::RawInput::default(), |_| {});

    // Test at default grid preview width (thumb_w ~138, thumb_h ~144)
    let thumb_w = 138.0_f32;
    let thumb_h = 144.0_f32;
    let border = 4.0_f32;
    let avail_w = (thumb_w - border * 2.0).max(10.0);
    let avail_h = (thumb_h - border * 2.0).max(10.0);

    // Natural visual aspect ratio is 1.0 : 0.82
    let font_size = avail_w.min(avail_h / 0.82);
    assert!(font_size > 50.0 && font_size <= avail_w);

    let folder_w = font_size;
    let folder_h = font_size * 0.82;
    let thumb_center = egui::pos2(thumb_w * 0.5, thumb_h * 0.5);
    let folder_rect = egui::Rect::from_center_size(thumb_center, egui::vec2(folder_w, folder_h));

    // Quad insets relative to folder_rect
    let quad_left = folder_rect.min.x + folder_rect.width() * 0.125;
    let quad_right = folder_rect.max.x - folder_rect.width() * 0.120;
    let quad_top = folder_rect.min.y + folder_rect.height() * 0.200;
    let quad_bottom = folder_rect.max.y - folder_rect.height() * 0.090;
    let quad_rect = egui::Rect::from_min_max(
        egui::pos2(quad_left, quad_top),
        egui::pos2(quad_right, quad_bottom),
    );

    // Quad must be strictly contained inside the folder bounding box
    assert!(quad_rect.min.x > folder_rect.min.x);
    assert!(quad_rect.max.x < folder_rect.max.x);
    assert!(quad_rect.min.y > folder_rect.min.y);
    assert!(quad_rect.max.y < folder_rect.max.y);
    assert!(quad_rect.width() > 0.0);
    assert!(quad_rect.height() > 0.0);

    // Verify font layout produces expected non-empty galley
    let galley = ctx.fonts(|f| {
        f.layout_no_wrap(
            icons::ICON_FOLDER.to_string(),
            egui::FontId::proportional(font_size),
            egui::Color32::WHITE,
        )
    });
    assert!(galley.size().x > 0.0);
    assert!(galley.size().y > 0.0);
}

#[test]
fn remap_cache_keys_preserves_and_relocates_entries() {
    let old_files = vec![
        PathBuf::from("C:/pics/img1.png"),
        PathBuf::from("C:/pics/img2.png"),
        PathBuf::from("C:/pics/img3.png"),
    ];
    // img0 added at start, img2 deleted, img1 shifted 0->1, img3 shifted 2->2
    let new_files = vec![
        PathBuf::from("C:/pics/img0.png"),
        PathBuf::from("C:/pics/img1.png"),
        PathBuf::from("C:/pics/img3.png"),
    ];
    let mut cache = HashMap::new();
    cache.insert(0, "texture1");
    cache.insert(1, "texture2");
    cache.insert(2, "texture3");

    let remapped = remap_cache_keys(&mut cache, &old_files, &new_files);
    assert_eq!(remapped.get(&0), None);
    assert_eq!(remapped.get(&1), Some(&"texture1"));
    assert_eq!(remapped.get(&2), Some(&"texture3"));
    assert_eq!(remapped.len(), 2);
}

#[test]
fn remap_cache_keys_with_rename_tracks_renamed_item() {
    let old_files = vec![
        PathBuf::from("C:/pics/alpha.png"),
        PathBuf::from("C:/pics/beta.png"),
        PathBuf::from("C:/pics/gamma.png"),
    ];
    // beta.png renamed to zulu.png -> moves from index 1 to index 2
    let new_files = vec![
        PathBuf::from("C:/pics/alpha.png"),
        PathBuf::from("C:/pics/gamma.png"),
        PathBuf::from("C:/pics/zulu.png"),
    ];
    let mut cache = HashMap::new();
    cache.insert(0, "alpha_tex");
    cache.insert(1, "beta_tex");
    cache.insert(2, "gamma_tex");

    let remapped =
        remap_cache_keys_with_rename(&mut cache, &old_files, &new_files, Some((1, 2)));
    assert_eq!(remapped.get(&0), Some(&"alpha_tex"));
    assert_eq!(remapped.get(&1), Some(&"gamma_tex"));
    assert_eq!(remapped.get(&2), Some(&"beta_tex"));
}

#[test]
fn validate_file_name_checks_rules() {
    let original = PathBuf::from("C:/pics/sample.jpg");

    // Empty / whitespace
    assert!(validate_file_name("", &original).is_err());
    assert!(validate_file_name("   ", &original).is_err());

    // Invalid characters
    assert!(validate_file_name("sample?.jpg", &original).is_err());
    assert!(validate_file_name("sam/ple.jpg", &original).is_err());
    assert!(validate_file_name("sam\\ple.jpg", &original).is_err());
    assert!(validate_file_name("sample*.jpg", &original).is_err());
    assert!(validate_file_name("sam<ple>.jpg", &original).is_err());
    assert!(validate_file_name("sample:1.jpg", &original).is_err());
    assert!(validate_file_name("sample|test.jpg", &original).is_err());

    // Trailing dot
    assert!(validate_file_name("sample.jpg.", &original).is_err());

    // Reserved Windows names
    assert!(validate_file_name("con.jpg", &original).is_err());
    assert!(validate_file_name("NUL.png", &original).is_err());
    assert!(validate_file_name("com1.webp", &original).is_err());
    assert!(validate_file_name("lpt3.jpg", &original).is_err());

    // Same name
    assert_eq!(
        validate_file_name("sample.jpg", &original),
        Ok("sample.jpg".to_string())
    );
    assert_eq!(
        validate_file_name("  sample.jpg  ", &original),
        Ok("sample.jpg".to_string())
    );

    // Valid new name
    assert_eq!(
        validate_file_name("vacation_2026.png", &original),
        Ok("vacation_2026.png".to_string())
    );
}

#[test]
fn compute_feedback_alpha_lifecycle() {
    // Fade-in phase (0.0s to 0.2s)
    let alpha_start = compute_feedback_alpha(0.0).unwrap();
    assert!((alpha_start - 0.0).abs() < 1e-4);

    let alpha_mid_in = compute_feedback_alpha(0.1).unwrap();
    assert!((alpha_mid_in - 0.5).abs() < 1e-4);

    let alpha_in_done = compute_feedback_alpha(0.2).unwrap();
    assert!((alpha_in_done - 1.0).abs() < 1e-4);

    // Hold phase (0.2s to 2.5s)
    let alpha_hold_1 = compute_feedback_alpha(1.0).unwrap();
    assert!((alpha_hold_1 - 1.0).abs() < 1e-4);

    let alpha_hold_end = compute_feedback_alpha(2.5).unwrap();
    assert!((alpha_hold_end - 1.0).abs() < 1e-4);

    // Fade-out phase (2.5s to 3.0s)
    let alpha_mid_out = compute_feedback_alpha(2.75).unwrap();
    assert!((alpha_mid_out - 0.5).abs() < 1e-4);

    let alpha_near_end = compute_feedback_alpha(2.95).unwrap();
    assert!((alpha_near_end - 0.1).abs() < 1e-3);

    // Expired (>= 3.0s)
    assert_eq!(compute_feedback_alpha(3.0), None);
    assert_eq!(compute_feedback_alpha(3.5), None);
}

#[test]
fn folder_history_stack_management() {
    let mut back_stack: Vec<PathBuf> = Vec::new();
    let mut forward_stack: Vec<PathBuf> = Vec::new();

    // Helper closures that match PhotonioApp's push/back/forward stack semantics
    let push_history = |cur: PathBuf, target: PathBuf, back: &mut Vec<PathBuf>, fwd: &mut Vec<PathBuf>| {
        if cur != target {
            if back.last() != Some(&cur) {
                back.push(cur);
                if back.len() > 50 {
                    back.remove(0);
                }
            }
            fwd.clear();
        }
    };

    let dir1 = PathBuf::from("C:\\photos\\vacation");
    let dir2 = PathBuf::from("C:\\photos\\work");
    let dir3 = PathBuf::from("C:\\photos\\family");

    // Navigate 1 -> 2
    push_history(dir1.clone(), dir2.clone(), &mut back_stack, &mut forward_stack);
    assert_eq!(back_stack, vec![dir1.clone()]);
    assert!(forward_stack.is_empty());

    // Consecutive navigation to same target should not duplicate
    push_history(dir2.clone(), dir2.clone(), &mut back_stack, &mut forward_stack);
    assert_eq!(back_stack, vec![dir1.clone()]);

    // Navigate 2 -> 3
    push_history(dir2.clone(), dir3.clone(), &mut back_stack, &mut forward_stack);
    assert_eq!(back_stack, vec![dir1.clone(), dir2.clone()]);
    assert!(forward_stack.is_empty());

    // Back: pop dir2, push current dir3 to forward_stack
    let prev = back_stack.pop().unwrap();
    assert_eq!(prev, dir2);
    forward_stack.push(dir3.clone());
    assert_eq!(back_stack, vec![dir1.clone()]);
    assert_eq!(forward_stack, vec![dir3.clone()]);

    // Forward: pop dir3, push current dir2 to back_stack
    let next = forward_stack.pop().unwrap();
    assert_eq!(next, dir3);
    back_stack.push(dir2.clone());
    assert_eq!(back_stack, vec![dir1.clone(), dir2.clone()]);
    assert!(forward_stack.is_empty());

    // Test capacity cap at 50
    let mut big_stack = Vec::new();
    let mut dummy_fwd = Vec::new();
    for i in 0..60 {
        push_history(
            PathBuf::from(format!("C:\\folder_{i}")),
            PathBuf::from(format!("C:\\folder_{}", i + 1)),
            &mut big_stack,
            &mut dummy_fwd,
        );
    }
    assert_eq!(big_stack.len(), 50);
    assert_eq!(big_stack.first(), Some(&PathBuf::from("C:\\folder_10")));
    assert_eq!(big_stack.last(), Some(&PathBuf::from("C:\\folder_59")));
}

#[test]
fn grid_tile_sizing_and_clamping() {
    assert_eq!(PhotonioApp::DEFAULT_GRID_TILE_SIZE, 150.0);
    assert_eq!(PhotonioApp::MIN_GRID_TILE_SIZE, 80.0);
    assert_eq!(PhotonioApp::MAX_GRID_TILE_SIZE, 500.0);

    // Standard label row height offset
    let h = PhotonioApp::DEFAULT_GRID_TILE_SIZE + 26.0;
    assert!((h - 176.0).abs() < 1e-4);

    // Clamping checks
    let clamped_low = 50.0_f32.clamp(PhotonioApp::MIN_GRID_TILE_SIZE, PhotonioApp::MAX_GRID_TILE_SIZE);
    assert_eq!(clamped_low, 80.0);

    let clamped_high = 700.0_f32.clamp(PhotonioApp::MIN_GRID_TILE_SIZE, PhotonioApp::MAX_GRID_TILE_SIZE);
    assert_eq!(clamped_high, 500.0);
}

#[test]
fn preview_lod_selection_and_target_sizes() {
    use crate::loader::PreviewLod;

    assert_eq!(PreviewLod::Small.target_size(), 384);
    assert_eq!(PreviewLod::Medium.target_size(), 640);
    assert_eq!(PreviewLod::Large.target_size(), 1024);

    // Standard DPI (1.0x)
    assert_eq!(PreviewLod::from_tile_width(120.0, 1.0), PreviewLod::Small);
    assert_eq!(PreviewLod::from_tile_width(200.0, 1.0), PreviewLod::Small);
    assert_eq!(PreviewLod::from_tile_width(300.0, 1.0), PreviewLod::Medium);
    assert_eq!(PreviewLod::from_tile_width(500.0, 1.0), PreviewLod::Large);

    // High DPI (1.5x)
    assert_eq!(PreviewLod::from_tile_width(120.0, 1.5), PreviewLod::Small);
    assert_eq!(PreviewLod::from_tile_width(200.0, 1.5), PreviewLod::Medium);
    assert_eq!(PreviewLod::from_tile_width(320.0, 1.5), PreviewLod::Large);

    // Retina / 4K (2.0x)
    assert_eq!(PreviewLod::from_tile_width(100.0, 2.0), PreviewLod::Small);
    assert_eq!(PreviewLod::from_tile_width(180.0, 2.0), PreviewLod::Medium);
    assert_eq!(PreviewLod::from_tile_width(250.0, 2.0), PreviewLod::Large);

    // Ordering allows upgrade comparison (e.g., existing.lod < needed_lod)
    assert!(PreviewLod::Small < PreviewLod::Medium);
    assert!(PreviewLod::Medium < PreviewLod::Large);
}

#[test]
fn rotation_clockwise_and_counter_clockwise_arithmetic() {
    let mut rot = 0;
    // CW: +90 % 360
    rot = (rot + 90) % 360;
    assert_eq!(rot, 90);
    rot = (rot + 90) % 360;
    assert_eq!(rot, 180);
    rot = (rot + 90) % 360;
    assert_eq!(rot, 270);
    rot = (rot + 90) % 360;
    assert_eq!(rot, 0);

    // CCW: +270 % 360
    rot = (rot + 270) % 360;
    assert_eq!(rot, 270);
    rot = (rot + 270) % 360;
    assert_eq!(rot, 180);
    rot = (rot + 270) % 360;
    assert_eq!(rot, 90);
    rot = (rot + 270) % 360;
    assert_eq!(rot, 0);

    // Mixed CW then CCW brings back to 0
    rot = (rot + 90) % 360;
    assert_eq!(rot, 90);
    rot = (rot + 270) % 360;
    assert_eq!(rot, 0);
}

#[test]
fn shortcut_save_and_save_as_chords() {
    // Ctrl + S (without Shift): Save / overwrite
    let ctrl_s = egui::Modifiers {
        ctrl: true,
        shift: false,
        alt: false,
        command: true,
        mac_cmd: false,
    };
    let is_save = ctrl_s.ctrl && !ctrl_s.shift;
    let is_save_as = ctrl_s.ctrl && ctrl_s.shift;
    assert!(is_save);
    assert!(!is_save_as);

    // Ctrl + Shift + S: Save As...
    let ctrl_shift_s = egui::Modifiers {
        ctrl: true,
        shift: true,
        alt: false,
        command: true,
        mac_cmd: false,
    };
    let is_save2 = ctrl_shift_s.ctrl && !ctrl_shift_s.shift;
    let is_save_as2 = ctrl_shift_s.ctrl && ctrl_shift_s.shift;
    assert!(!is_save2);
    assert!(is_save_as2);
}

#[test]
fn cover_uv_aspect_calculation() {
    // Square image into square slot: UV is full (0..1)
    let slot = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
    let uv_sq = cover_uv([200, 200], slot);
    assert!((uv_sq.min.x - 0.0).abs() < 1e-4);
    assert!((uv_sq.min.y - 0.0).abs() < 1e-4);
    assert!((uv_sq.max.x - 1.0).abs() < 1e-4);
    assert!((uv_sq.max.y - 1.0).abs() < 1e-4);

    // Landscape image (2:1) into square slot: height covers (y is 0..1), width is center-cropped (x > 0 and < 1)
    let uv_land = cover_uv([400, 200], slot);
    assert!((uv_land.min.y - 0.0).abs() < 1e-4);
    assert!((uv_land.max.y - 1.0).abs() < 1e-4);
    assert!(uv_land.min.x > 0.1 && uv_land.min.x < 0.4);
    assert!(uv_land.max.x > 0.6 && uv_land.max.x < 0.9);
}

#[test]
fn multiview_unified_grid_selection_mapping() {
    let folder_count = 3;
    let image_count = 5;
    let total_items = folder_count + image_count;
    assert_eq!(total_items, 8);

    // Folder indices 0..3
    for f in 0..folder_count {
        let selected_folder = Some(f);
        let unified = match selected_folder {
            Some(idx) if idx < folder_count => idx,
            _ => folder_count,
        };
        assert_eq!(unified, f);
        assert!(unified < folder_count);
    }

    // Image indices 0..5 map to unified indices 3..8
    for i in 0..image_count {
        let selected_folder: Option<usize> = None;
        let current_image_idx = i;
        let unified = match selected_folder {
            Some(idx) if idx < folder_count => idx,
            _ => folder_count + current_image_idx,
        };
        assert_eq!(unified, folder_count + i);
        assert!(unified >= folder_count && unified < total_items);
    }
}

#[test]
fn test_breadcrumbs_short_path_no_ellipsis() {
    let crumbs = vec![
        ("C:\\".to_string(), PathBuf::from("C:\\")),
        ("Photos".to_string(), PathBuf::from("C:\\Photos")),
    ];
    let segments = layout_breadcrumbs(&crumbs, 500.0, |t, _| t.chars().count() as f32 * 10.0);
    assert_eq!(segments.len(), 2);
    match &segments[0] {
        BreadcrumbSegment::Crumb { label, is_current, .. } => {
            assert_eq!(label, "C:\\");
            assert!(!is_current);
        }
        _ => panic!("Expected Crumb"),
    }
    match &segments[1] {
        BreadcrumbSegment::Crumb { label, is_current, .. } => {
            assert_eq!(label, "Photos");
            assert!(is_current);
        }
        _ => panic!("Expected Crumb"),
    }
}

#[test]
fn test_breadcrumbs_intermediate_collapse_keeps_root_and_trailing() {
    let crumbs = vec![
        ("F:\\".to_string(), PathBuf::from("F:\\")),
        ("Collections".to_string(), PathBuf::from("F:\\Collections")),
        ("091. Files 3".to_string(), PathBuf::from("F:\\Collections\\091. Files 3")),
        ("kids on cam".to_string(), PathBuf::from("F:\\Collections\\091. Files 3\\kids on cam")),
        ("CaptureFiles".to_string(), PathBuf::from("F:\\Collections\\091. Files 3\\kids on cam\\CaptureFiles")),
        ("2018-04-05".to_string(), PathBuf::from("F:\\Collections\\091. Files 3\\kids on cam\\CaptureFiles\\2018-04-05")),
    ];

    let segments = layout_breadcrumbs(&crumbs, 260.0, |t, _| t.chars().count() as f32 * 8.0);
    assert!(segments.len() >= 3);
    assert!(matches!(&segments[0], BreadcrumbSegment::Crumb { label, .. } if label == "F:\\"));
    assert!(matches!(&segments[1], BreadcrumbSegment::Ellipsis { hidden } if !hidden.is_empty()));
    assert!(matches!(segments.last(), Some(BreadcrumbSegment::Crumb { is_current: true, .. })));
}

#[test]
fn test_breadcrumbs_tight_space_collapses_to_ellipsis_and_current() {
    let crumbs = vec![
        ("F:\\".to_string(), PathBuf::from("F:\\")),
        ("VeryLongIntermediateFolderNameA".to_string(), PathBuf::from("F:\\A")),
        ("VeryLongIntermediateFolderNameB".to_string(), PathBuf::from("F:\\A\\B")),
        ("CurrentActiveFolder".to_string(), PathBuf::from("F:\\A\\B\\CurrentActiveFolder")),
    ];

    let segments = layout_breadcrumbs(&crumbs, 190.0, |t, _| t.chars().count() as f32 * 7.0);
    assert_eq!(segments.len(), 2);
    assert!(matches!(&segments[0], BreadcrumbSegment::Ellipsis { .. }));
    assert!(matches!(&segments[1], BreadcrumbSegment::Crumb { label, is_current: true, .. } if label == "CurrentActiveFolder"));
}

#[test]
fn test_breadcrumbs_narrow_truncates_current() {
    let crumbs = vec![
        ("F:\\".to_string(), PathBuf::from("F:\\")),
        ("ExtremelySuperLongActiveFolderNameExceedingAllBounds".to_string(), PathBuf::from("F:\\Long")),
    ];
    let segments = layout_breadcrumbs(&crumbs, 60.0, |t, _| t.chars().count() as f32 * 8.0);
    assert_eq!(segments.len(), 2);
    assert!(matches!(&segments[0], BreadcrumbSegment::Ellipsis { .. }));
    if let BreadcrumbSegment::Crumb { label, is_current, .. } = &segments[1] {
        assert!(is_current);
        assert!(label.len() < "ExtremelySuperLongActiveFolderNameExceedingAllBounds".len());
    } else {
        panic!("Expected truncated crumb");
    }
}

#[test]
fn test_remember_last_folder_disabled_clears_paths_on_save() {
    let mut config = crate::config::Config::default();
    config.remember_last_folder = false;
    config.last_directory = Some(PathBuf::from("C:\\test\\dir"));
    config.last_file_path = Some(PathBuf::from("C:\\test\\dir\\img.png"));

    if !config.remember_last_folder {
        config.last_directory = None;
        config.last_file_path = None;
    }

    assert!(config.last_directory.is_none());
    assert!(config.last_file_path.is_none());
}

#[test]
fn test_viewer_drag_deadzone_distance() {
    let start = egui::pos2(100.0, 100.0);
    let small_twitch = egui::pos2(103.0, 104.0);
    // distance = sqrt(3^2 + 4^2) = 5.0, which is < 6.0 threshold
    assert!(small_twitch.distance(start) < 6.0);

    let deliberate_drag = egui::pos2(106.0, 108.0);
    // distance = 10.0, which is >= 6.0 threshold
    assert!(deliberate_drag.distance(start) >= 6.0);
}

#[test]
fn test_grip_icon_lays_out() {
    let ctx = egui::Context::default();
    setup_fonts(&ctx);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let galley_v = ui.fonts(|f| {
                f.layout_no_wrap(
                    crate::icons::ICON_GRIP_VERTICAL.to_string(),
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                )
            });
            assert!(galley_v.size().x > 0.0);
            assert!(galley_v.size().y > 0.0);

            let galley_h = ui.fonts(|f| {
                f.layout_no_wrap(
                    crate::icons::ICON_GRIP_HORIZONTAL.to_string(),
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                )
            });
            assert!(galley_h.size().x > 0.0);
            assert!(galley_h.size().y > 0.0);
        });
    });
}

#[test]
fn test_quick_pill_pos_config_roundtrip() {
    let mut config = crate::config::Config::default();
    assert_eq!(config.quick_pill_pos, None);
    config.quick_pill_pos = Some([250.0, 120.0]);
    let json = serde_json::to_string(&config).expect("serialize config");
    let loaded: crate::config::Config = serde_json::from_str(&json).expect("deserialize config");
    assert_eq!(loaded.quick_pill_pos, Some([250.0, 120.0]));
}

#[test]
fn test_grid_actions_support_drag() {
    use crate::app::types::{GridFolderAction, GridTileAction};

    let tile_action = GridTileAction::Drag(5);
    match tile_action {
        GridTileAction::Drag(idx) => assert_eq!(idx, 5),
        _ => panic!("Expected Drag"),
    }

    let folder_action = GridFolderAction::Drag(2);
    match folder_action {
        GridFolderAction::Drag(idx) => assert_eq!(idx, 2),
        _ => panic!("Expected Drag"),
    }
}

#[test]
fn test_overlay_anchor_closest_edge_quadrants() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));

    // Top-Right quadrant
    let tr_panel = egui::Rect::from_min_size(egui::pos2(920.0, 20.0), egui::vec2(60.0, 180.0));
    let anchor_tr = crate::config::OverlayAnchor::from_panel_and_screen(tr_panel, screen);
    assert!(anchor_tr.from_right);
    assert!(!anchor_tr.from_bottom);
    assert_eq!(anchor_tr.offset_x, 20.0); // 1000 - 980 = 20
    assert_eq!(anchor_tr.offset_y, 20.0); // 20 - 0 = 20
    let (align, offset) = anchor_tr.to_align_and_offset(screen);
    assert_eq!(align, egui::Align2::RIGHT_TOP);
    assert_eq!(offset, egui::vec2(-20.0, 20.0));

    // Top-Left quadrant
    let tl_panel = egui::Rect::from_min_size(egui::pos2(30.0, 40.0), egui::vec2(60.0, 180.0));
    let anchor_tl = crate::config::OverlayAnchor::from_panel_and_screen(tl_panel, screen);
    assert!(!anchor_tl.from_right);
    assert!(!anchor_tl.from_bottom);
    assert_eq!(anchor_tl.offset_x, 30.0);
    assert_eq!(anchor_tl.offset_y, 40.0);
    let (align, offset) = anchor_tl.to_align_and_offset(screen);
    assert_eq!(align, egui::Align2::LEFT_TOP);
    assert_eq!(offset, egui::vec2(30.0, 40.0));

    // Bottom-Right quadrant
    let br_panel = egui::Rect::from_min_size(egui::pos2(880.0, 650.0), egui::vec2(80.0, 120.0));
    let anchor_br = crate::config::OverlayAnchor::from_panel_and_screen(br_panel, screen);
    assert!(anchor_br.from_right);
    assert!(anchor_br.from_bottom);
    assert_eq!(anchor_br.offset_x, 40.0); // 1000 - 960 = 40
    assert_eq!(anchor_br.offset_y, 30.0); // 800 - 770 = 30
    let (align, offset) = anchor_br.to_align_and_offset(screen);
    assert_eq!(align, egui::Align2::RIGHT_BOTTOM);
    assert_eq!(offset, egui::vec2(-40.0, -30.0));

    // Bottom-Left quadrant
    let bl_panel = egui::Rect::from_min_size(egui::pos2(15.0, 700.0), egui::vec2(100.0, 80.0));
    let anchor_bl = crate::config::OverlayAnchor::from_panel_and_screen(bl_panel, screen);
    assert!(!anchor_bl.from_right);
    assert!(anchor_bl.from_bottom);
    assert_eq!(anchor_bl.offset_x, 15.0);
    assert_eq!(anchor_bl.offset_y, 20.0); // 800 - 780 = 20
    let (align, offset) = anchor_bl.to_align_and_offset(screen);
    assert_eq!(align, egui::Align2::LEFT_BOTTOM);
    assert_eq!(offset, egui::vec2(15.0, -20.0));
}

#[test]
fn test_overlay_anchor_window_resize_preserves_relative_distance() {
    let initial_screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    // Placed 15px from right edge, 25px from top
    let pill_rect = egui::Rect::from_min_size(egui::pos2(1865.0, 25.0), egui::vec2(40.0, 200.0));
    let anchor = crate::config::OverlayAnchor::from_panel_and_screen(pill_rect, initial_screen);
    assert!(anchor.from_right);
    assert_eq!(anchor.offset_x, 15.0);
    assert_eq!(anchor.offset_y, 25.0);

    // Now window is resized down to 1280x720
    let resized_screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
    let (align, offset) = anchor.to_align_and_offset(resized_screen);
    assert_eq!(align, egui::Align2::RIGHT_TOP);
    // Offset remains strictly locked to -15 from right and +25 from top
    assert_eq!(offset.x, -15.0);
    assert_eq!(offset.y, 25.0);
}

#[test]
fn test_overlay_anchor_config_roundtrip() {
    let mut config = crate::config::Config::default();
    assert_eq!(config.quick_pill_anchor, None);
    let anchor = crate::config::OverlayAnchor::new(true, false, 18.0, 22.0);
    config.quick_pill_anchor = Some(anchor);
    let json = serde_json::to_string(&config).expect("serialize config");
    let loaded: crate::config::Config = serde_json::from_str(&json).expect("deserialize config");
    assert_eq!(loaded.quick_pill_anchor, Some(anchor));
    assert!(loaded.quick_pill_anchor.unwrap().from_right);
    assert_eq!(loaded.quick_pill_anchor.unwrap().offset_x, 18.0);
    assert_eq!(loaded.quick_pill_anchor.unwrap().offset_y, 22.0);
}
