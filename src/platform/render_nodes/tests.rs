//! How a `/sys/class/drm` directory becomes render nodes, against directories built here.

use std::fs;
use std::path::PathBuf;

use super::{list_in, RenderNode, AMD};

#[test]
fn a_render_node_shows_its_vendor_and_memory_and_other_entries_are_ignored() {
    let drm = tempdir("drm");
    let device = drm.join("renderD128/device");
    fs::create_dir_all(&device).unwrap();
    fs::write(device.join("vendor"), "0x1002\n").unwrap();
    fs::write(device.join("mem_info_vram_total"), "536870912\n").unwrap();
    fs::write(device.join("mem_info_gtt_total"), "8589934592\n").unwrap();
    // A card node and a display connector are not render nodes.
    fs::create_dir_all(drm.join("card0/device")).unwrap();
    fs::create_dir_all(drm.join("card0-HDMI-A-1")).unwrap();
    assert_eq!(
        list_in(&drm),
        [RenderNode {
            vendor: Some(AMD),
            vram: Some(536_870_912),
            gtt: Some(8_589_934_592)
        }]
    );
}

#[test]
fn a_node_without_files_is_listed_with_nothing_known_and_no_directory_lists_none() {
    let drm = tempdir("bare");
    fs::create_dir_all(drm.join("renderD129")).unwrap();
    assert_eq!(
        list_in(&drm),
        [RenderNode {
            vendor: None,
            vram: None,
            gtt: None
        }]
    );
    assert!(list_in(&drm.join("missing")).is_empty());
}

fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sidevoice-render-nodes-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}
