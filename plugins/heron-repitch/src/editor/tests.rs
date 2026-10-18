//! Editor gesture smoke coverage mirrored from Revox.

use super::*;
use crate::RepitchParams;
use std::sync::Arc;
use truce::prelude::{Editor, Params};
use truce_iced::IcedEditor;

#[test]
#[ignore = "manual native 1× and 2× editor render evidence"]
fn render_editor_evidence() {
    let params = Arc::new(RepitchParams::default());
    let directory = std::path::Path::new("../../out/plugin-native");
    std::fs::create_dir_all(directory).expect("native evidence directory");
    for scale in [1, 2] {
        let size = EDITOR_SIZE;
        let mut editor = IcedEditor::<RepitchParams, RepitchUi>::new(params.clone(), size);
        editor.set_scale_factor(f64::from(scale));
        let erased: Arc<dyn Params> = params.clone();
        let (pixels, width, height) = editor.screenshot(erased).expect("native editor render");
        assert_eq!((width, height), (size.0 * scale, size.1 * scale));
        truce::core::screenshot::save_png(
            &directory.join(format!("repitch-{scale}x.png")),
            &pixels,
            width,
            height,
        );
    }
}
