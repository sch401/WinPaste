const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const ts = require('typescript');

function deferred() {
    let resolve;
    const promise = new Promise(r => { resolve = r; });
    return { promise, resolve };
}
function fixture() {
    const listeners = new Map();
    const calls = [];
    let visible = true;
    const monitor = { position: { x: 0, y: 0 }, size: { width: 1920, height: 1080 } };
    const main = { scaleFactor: async () => 1, isVisible: async () => visible };
    const preview = {
        setPosition: async () => {}, show: async () => { calls.push('show'); },
        setSize: async () => {}, hide: async () => { calls.push('hide'); }
    };
    const api = {
        '@tauri-apps/api/event': { listen: async (name, callback) => { listeners.set(name, callback); return () => {}; } },
        '@tauri-apps/api/core': { invoke: async (name) => { calls.push(name); } },
        '@tauri-apps/api/window': {
            PhysicalPosition: class {}, PhysicalSize: class {},
            getCurrentWindow: () => main, currentMonitor: async () => monitor, monitorFromPoint: async () => monitor
        },
        '@tauri-apps/api/webviewWindow': { WebviewWindow: { getByLabel: async () => preview } }
    };
    const source = fs.readFileSync('src/features/clipboard/lib/compactPreviewController.ts', 'utf8');
    const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText;
    const exports = {};
    vm.runInNewContext(compiled, { exports, require: name => api[name], setTimeout, clearTimeout, console });
    const controller = exports.compactPreviewController;
    controller.window = preview;
    controller.generation = 1;
    controller.pending = { generation: 1 };
    controller.resolveAnchorPhysical = async () => ({ left: 20, top: 20, width: 100, height: 40 });
    return { controller, preview, calls, listeners, setVisible: value => { visible = value; } };
}

test('closing the main window during positioning cancels preview display', async () => {
    const f = fixture(), gate = deferred();
    f.preview.setPosition = () => gate.promise;
    const task = f.controller.placeAndShow({}, { width: 320, height: 220 }, 1);
    await new Promise(setImmediate);
    await f.controller.hide();
    gate.resolve();
    await task;
    assert.equal(f.calls.includes('show'), false);
});

test('closing while show is pending hides the late preview and skips raising it', async () => {
    const f = fixture(), gate = deferred();
    f.preview.show = async () => { await gate.promise; f.calls.push('show'); };
    const task = f.controller.placeAndShow({}, { width: 320, height: 220 }, 1);
    await new Promise(setImmediate);
    await f.controller.hide();
    gate.resolve();
    await task;
    assert.equal(f.calls.at(-1), 'hide');
    assert.equal(f.calls.includes('raise_window_topmost'), false);
});

test('a hidden main window prevents a delayed hover request from displaying', async () => {
    const f = fixture();
    f.setVisible(false);
    await f.controller.placeAndShow({}, { width: 320, height: 220 }, 1);
    assert.equal(f.calls.includes('show'), false);
});

test('the backend hide event cancels the current request', async () => {
    const f = fixture();
    await f.controller.ensureLifecycleListeners();
    const revision = f.controller.revision;
    f.listeners.get('window-hidden')();
    await new Promise(setImmediate);
    assert.ok(f.controller.revision > revision);
    assert.equal(f.controller.pending, null);
    assert.ok(f.calls.includes('hide'));
});

test('leaving an older row preserves the current row preview', async () => {
    const f = fixture();
    f.controller.owner = 'new-row';
    await f.controller.hide('old-row');
    assert.equal(f.controller.revision, 1);
    assert.equal(f.calls.includes('hide'), false);
});
