use super::*;
use std::sync::Mutex;
use truce::core::{editor::EditorBridge, events::TransportInfo};

#[derive(Debug, PartialEq)]
enum HostEvent {
    Begin(u32),
    Set(u32, f64),
    End(u32),
}

struct Host {
    params: Arc<RevoxParams>,
    events: Mutex<Vec<HostEvent>>,
}
impl Host {
    fn new(params: Arc<RevoxParams>) -> Arc<Self> {
        Arc::new(Self {
            params,
            events: Mutex::new(Vec::new()),
        })
    }
    fn record(&self, event: HostEvent) {
        self.events.lock().unwrap().push(event);
    }
}
impl EditorBridge for Host {
    fn begin_edit(&self, id: u32) {
        self.record(HostEvent::Begin(id));
    }
    fn set_param(&self, id: u32, value: f64) {
        self.params.set_normalized(id, value);
        self.record(HostEvent::Set(id, value));
    }
    fn end_edit(&self, id: u32) {
        self.record(HostEvent::End(id));
    }
    fn request_resize(&self, _: u32, _: u32) -> bool {
        false
    }
    fn get_param(&self, id: u32) -> f64 {
        self.params.get_normalized(id).unwrap_or(0.0)
    }
    fn get_param_plain(&self, id: u32) -> f64 {
        self.params.get_plain(id).unwrap_or(0.0)
    }
    fn format_param(&self, _: u32) -> String {
        String::new()
    }
    fn get_meter(&self, _: u32) -> f32 {
        0.0
    }
    fn get_state(&self) -> Vec<u8> {
        Vec::new()
    }
    fn set_state(&self, _: Vec<u8>) {}
    fn transport(&self) -> Option<TransportInfo> {
        None
    }
}

fn update(
    ui: &mut RevoxUi,
    cache: &ParamCache<RevoxParams>,
    context: &PluginContext<RevoxParams>,
    edit: Edit,
) {
    let _ = ui.update(
        Message::Plugin(RevoxMessage::Edit(ui.revision, edit)),
        cache,
        context,
    );
}

#[test]
fn semitone_drag_quantizes_and_cancellation_balances_the_host_gesture() {
    let params = Arc::new(RevoxParams::default());
    let host = Host::new(params.clone());
    let context = PluginContext::new(host.clone(), params.clone());
    let cache = ParamCache::new(params.clone());
    let mut ui = RevoxUi::new(params.clone());
    update(&mut ui, &cache, &context, Edit::Begin(Field::Semitones));
    update(
        &mut ui,
        &cache,
        &context,
        Edit::Change(Field::Semitones, 3.7),
    );
    assert_eq!(params.get_plain(0), Some(4.0));
    update(&mut ui, &cache, &context, Edit::Cancel);
    update(
        &mut ui,
        &cache,
        &context,
        Edit::Change(Field::Semitones, 10.0),
    );
    assert_eq!(params.get_plain(0), Some(0.0));
    assert_eq!(
        *host.events.lock().unwrap(),
        vec![
            HostEvent::Begin(0),
            HostEvent::Set(0, 16.0 / 24.0),
            HostEvent::Set(0, 0.5),
            HostEvent::End(0)
        ]
    );
}

#[test]
fn numeric_entry_is_integral_and_cents_do_not_overwrite_the_semitone_offset() {
    let params = Arc::new(RevoxParams::default());
    params.set_normalized(0, Field::Semitones.normalized(3.0));
    let host = Host::new(params.clone());
    let context = PluginContext::new(host.clone(), params.clone());
    let cache = ParamCache::new(params.clone());
    let mut ui = RevoxUi::new(params.clone());
    update(&mut ui, &cache, &context, Edit::Numeric(Field::Cents));
    update(&mut ui, &cache, &context, Edit::Draft("12.5".to_owned()));
    update(&mut ui, &cache, &context, Edit::Submit);
    assert!(ui.invalid);
    assert!(host.events.lock().unwrap().is_empty());
    update(&mut ui, &cache, &context, Edit::Draft("-25".to_owned()));
    update(&mut ui, &cache, &context, Edit::Submit);
    assert_eq!(params.get_plain(0), Some(3.0));
    assert_eq!(params.get_plain(1), Some(-25.0));
    assert_eq!(
        *host.events.lock().unwrap(),
        vec![
            HostEvent::Begin(1),
            HostEvent::Set(1, 0.375),
            HostEvent::End(1)
        ]
    );
    for draft in ["NaN", "inf", "101", "-101", ""] {
        assert_eq!(parse_value(Field::Cents, draft), None);
    }
}

#[test]
fn recall_discards_old_drag_and_numeric_events_and_closure_revokes_callbacks() {
    let params = Arc::new(RevoxParams::default());
    let host = Host::new(params.clone());
    let gate = Arc::new(lifecycle::Gate::new(params.clone(), host.clone()));
    let context = PluginContext::new(gate.clone(), params.clone());
    let cache = ParamCache::new(params.clone());
    let mut ui = RevoxUi::new(params.clone());
    update(&mut ui, &cache, &context, Edit::Begin(Field::Semitones));
    let old_revision = ui.revision;
    gate.end_edits();
    params.set_normalized(0, Field::Semitones.normalized(-4.0));
    let _ = ui.view(&cache);
    assert!(!ui.needs_redraw());
    params.editor_state.invalidate();
    assert!(ui.needs_redraw());
    let _ = ui.view(&cache);
    assert!(
        !ui.needs_redraw(),
        "recall must not leave the editor repainting while idle"
    );
    let _ = ui.update(
        Message::Plugin(RevoxMessage::Edit(
            old_revision,
            Edit::Change(Field::Semitones, 10.0),
        )),
        &cache,
        &context,
    );
    assert_eq!(params.get_plain(0), Some(-4.0));
    assert!(ui.active.is_none());
    update(&mut ui, &cache, &context, Edit::Numeric(Field::Cents));
    update(&mut ui, &cache, &context, Edit::Draft("50".to_owned()));
    let numeric_revision = ui.revision;
    params.editor_state.invalidate();
    let _ = ui.update(
        Message::Plugin(RevoxMessage::Edit(numeric_revision, Edit::Submit)),
        &cache,
        &context,
    );
    assert!(ui.numeric.is_none());
    assert_eq!(params.get_plain(1), Some(0.0));
    update(&mut ui, &cache, &context, Edit::Begin(Field::Cents));
    gate.close();
    context.set_param(1_u32, 1.0);
    context.end_edit(1_u32);
    assert_eq!(params.get_plain(1), Some(0.0));
    assert_eq!(
        *host.events.lock().unwrap(),
        vec![
            HostEvent::Begin(0),
            HostEvent::End(0),
            HostEvent::Begin(1),
            HostEvent::End(1)
        ]
    );
}

#[test]
fn keyboard_selects_each_pitch_control_and_keeps_fine_and_large_steps_integral() {
    let params = Arc::new(RevoxParams::default());
    let host = Host::new(params.clone());
    let context = PluginContext::new(host.clone(), params.clone());
    let mut ui = RevoxUi::new(params.clone());
    let key = |named, modifiers| keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(named),
        modified_key: keyboard::Key::Named(named),
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    };
    use keyboard::{Modifiers, key::Named};
    ui.key(key(Named::Tab, Modifiers::empty()), &context);
    ui.key(key(Named::ArrowUp, Modifiers::empty()), &context);
    ui.key(key(Named::ArrowUp, Modifiers::SHIFT), &context);
    assert_eq!(params.get_plain(0), Some(4.0));
    ui.key(key(Named::Tab, Modifiers::empty()), &context);
    ui.key(key(Named::ArrowDown, Modifiers::empty()), &context);
    ui.key(key(Named::ArrowDown, Modifiers::SHIFT), &context);
    assert_eq!(params.get_plain(1), Some(-11.0));
    ui.key(key(Named::Home, Modifiers::empty()), &context);
    assert_eq!(params.get_plain(1), Some(0.0));
    assert_eq!(params.get_plain(0), Some(4.0));
    let events = host.events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, HostEvent::Begin(_)))
            .count(),
        5
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, HostEvent::End(_)))
            .count(),
        5
    );
}

#[test]
#[ignore = "manual native 1× and 2× editor render evidence"]
fn render_editor_evidence() {
    let params = Arc::new(RevoxParams::default());
    let geometry = crate::stream::Geometry::new(48_000.0).expect("supported sample rate");
    params.status.configure(48_000.0, geometry.latency as u32);
    params.status.set(ProcessingState::Ready);
    params.set_normalized(0, Field::Semitones.normalized(3.0));
    params.set_normalized(1, Field::Cents.normalized(50.0));
    let directory = std::path::Path::new("../../out/plugin-native");
    std::fs::create_dir_all(directory).expect("native evidence directory");
    for scale in [1, 2] {
        let mut editor = create(params.clone());
        editor.set_scale_factor(f64::from(scale));
        let erased: Arc<dyn Params> = params.clone();
        let (pixels, width, height) = editor.screenshot(erased).expect("native editor render");
        assert_eq!(
            (width, height),
            (EDITOR_SIZE.0 * scale, EDITOR_SIZE.1 * scale)
        );
        truce::core::screenshot::save_png(
            &directory.join(format!("revox-{scale}x.png")),
            &pixels,
            width,
            height,
        );
    }
}
