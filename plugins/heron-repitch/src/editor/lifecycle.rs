//! Balance edits and revoke callbacks before Iced's asynchronous native teardown.

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
use truce::{
    core::{
        editor::{Editor, EditorBridge, PluginContext, RawWindowHandle},
        events::TransportInfo,
    },
    prelude::Params,
};

use crate::RepitchParams;

struct Host {
    bridge: Arc<dyn EditorBridge>,
    active: BTreeSet<u32>,
}

pub(super) struct Gate {
    host: Mutex<Option<Host>>,
    params: Arc<RepitchParams>,
}

impl Gate {
    pub(super) fn new(params: Arc<RepitchParams>, bridge: Arc<dyn EditorBridge>) -> Self {
        Self {
            host: Mutex::new(Some(Host {
                bridge,
                active: BTreeSet::new(),
            })),
            params,
        }
    }
    fn call<T>(&self, action: impl FnOnce(&mut Host) -> T) -> Option<T> {
        self.host
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_mut()
            .map(action)
    }
    pub(super) fn end_edits(&self) {
        self.call(|host| {
            for id in std::mem::take(&mut host.active) {
                host.bridge.end_edit(id);
            }
        });
    }
    pub(super) fn close(&self) {
        if let Some(host) = self
            .host
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            for id in host.active {
                host.bridge.end_edit(id);
            }
        }
    }
}

impl EditorBridge for Gate {
    fn begin_edit(&self, id: u32) {
        self.call(|host| {
            if host.active.insert(id) {
                host.bridge.begin_edit(id);
            }
        });
    }
    fn set_param(&self, id: u32, value: f64) {
        self.call(|host| host.bridge.set_param(id, value));
    }
    fn end_edit(&self, id: u32) {
        self.call(|host| {
            if host.active.remove(&id) {
                host.bridge.end_edit(id);
            }
        });
    }
    fn request_resize(&self, width: u32, height: u32) -> bool {
        self.call(|host| host.bridge.request_resize(width, height))
            .unwrap_or(false)
    }
    fn get_param(&self, id: u32) -> f64 {
        self.params.get_normalized(id).unwrap_or(0.0)
    }
    fn get_param_plain(&self, id: u32) -> f64 {
        self.params.get_plain(id).unwrap_or(0.0)
    }
    fn format_param(&self, id: u32) -> String {
        self.params
            .format_value(id, self.get_param_plain(id))
            .unwrap_or_default()
    }
    fn get_meter(&self, id: u32) -> f32 {
        self.call(|host| host.bridge.get_meter(id)).unwrap_or(0.0)
    }
    fn get_state(&self) -> Vec<u8> {
        self.call(|host| host.bridge.get_state())
            .unwrap_or_default()
    }
    fn set_state(&self, data: Vec<u8>) {
        self.call(|host| host.bridge.set_state(data));
    }
    fn transport(&self) -> Option<TransportInfo> {
        self.call(|host| host.bridge.transport()).flatten()
    }
}

pub(super) struct RepitchEditor {
    params: Arc<RepitchParams>,
    inner: Box<dyn Editor>,
    gate: Option<Arc<Gate>>,
}

impl RepitchEditor {
    pub(super) fn new(params: Arc<RepitchParams>, inner: Box<dyn Editor>) -> Self {
        Self {
            params,
            inner,
            gate: None,
        }
    }
    fn revoke(&mut self) {
        if let Some(gate) = self.gate.take() {
            gate.close();
            self.params.editor_state.invalidate();
        }
    }
}

impl Editor for RepitchEditor {
    fn size(&self) -> (u32, u32) {
        self.inner.size()
    }
    fn open(&mut self, parent: RawWindowHandle, context: PluginContext) {
        self.revoke();
        let gate = Arc::new(Gate::new(self.params.clone(), context.bridge().clone()));
        let gated = PluginContext::new(gate.clone(), context.params().clone());
        self.gate = Some(gate);
        self.inner.open(parent, gated);
    }
    fn close(&mut self) {
        self.revoke();
        self.inner.close();
    }
    fn idle(&mut self) {
        self.inner.idle();
    }
    fn set_size(&mut self, width: u32, height: u32) -> bool {
        self.inner.set_size(width, height)
    }
    fn set_scale_factor(&mut self, factor: f64) {
        self.inner.set_scale_factor(factor);
    }
    fn set_uses_system_scale(&mut self, yes: bool) {
        self.inner.set_uses_system_scale(yes);
    }
    fn state_changed(&mut self) {
        if let Some(gate) = &self.gate {
            gate.end_edits();
        }
        self.params.editor_state.invalidate();
        self.inner.state_changed();
    }
    fn screenshot(&mut self, params: Arc<dyn Params>) -> Option<(Vec<u8>, u32, u32)> {
        self.inner.screenshot(params)
    }
}

impl Drop for RepitchEditor {
    fn drop(&mut self) {
        self.revoke();
        self.inner.close();
    }
}
