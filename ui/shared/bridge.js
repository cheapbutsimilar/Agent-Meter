/* Maps `window.agentMeter` onto Tauri commands & events.
 * In a plain browser (live preview / screenshots) scripts/mock-api.js provides it instead. */
(function () {
  'use strict';
  if (window.agentMeter) return;
  const T = window.__TAURI__;
  if (!T) {
    console.error('AgentMeter: Tauri API not available');
    return;
  }
  const invoke = T.core.invoke;
  const listen = T.event.listen;

  window.agentMeter = {
    init: () => invoke('app_init'),
    setSettings: (patch) => invoke('settings_set', { patch }),
    refresh: () => invoke('usage_refresh'),
    updateCheck: () => invoke('update_check'),
    updateInstall: () => invoke('update_install'),
    updateLater: () => invoke('update_later'),
    changelog: () => invoke('changelog'),
    setupStatus: () => invoke('setup_status'),
    setupConnect: (agent) => invoke('setup_connect', { agent }),
    antigravityPick: () => invoke('antigravity_pick'),
    openDashboard: (page) => invoke('open_dashboard', { page: page || null }),
    closeWindow: () => invoke('close_window'),
    minimizeWindow: () => invoke('minimize_window'),
    toggleMaximize: () => invoke('toggle_maximize'),
    widgetResize: (width, height) => invoke('widget_resize', { width, height }),
    widgetMenu: () => invoke('widget_menu'),
    dragStart: (x, y) => invoke('drag_start', { x, y }),
    dragMove: (x, y) => invoke('drag_move', { x, y }),
    dragEnd: () => invoke('drag_end'),
    hotkeysSuspend: (suspend) => invoke('hotkeys_suspend', { suspend }),
    exportCsv: () => invoke('export_csv'),
    openDataDir: () => invoke('open_data_dir'),
    openUrl: (url) => invoke('open_url', { url }),
    memeSound: () => invoke('meme_sound'),
    qwenPick: () => invoke('qwen_pick'),
    quit: () => invoke('quit_app'),
    on: (channel, cb) => {
      let unlisten = null;
      listen(channel, (e) => cb(e.payload)).then((fn) => (unlisten = fn));
      return () => unlisten && unlisten();
    },
  };
})();
