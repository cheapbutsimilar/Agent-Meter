/* Widget window: docked on the taskbar or floating; Page Up switches (see Rust hotkeys). */
(async () => {
  'use strict';
  const R = window.AM_RENDER;
  const api = window.agentMeter;
  const root = document.getElementById('root');

  const S = await api.init();
  let system = S.system || { cpu: 0, ram: 0 };
  let taskbarHeight = S.taskbarHeight || 40;
  let lastSize = '';
  let dragging = false;
  let dragged = false; // a drag, so the click that follows it is not a poke
  let memeTimer = 0;
  let moodTimer = 0;
  let chaosTimer = 0;

  // macOS has no taskbar: there the strip always floats (canDock from the Rust side)
  const docked = (s) => S.canDock !== false && s.docked !== false;

  function render() {
    if (dragging) return;
    const s = S.settings;
    window.AM_I18N.setLang(s.language);
    window.AM_UTIL.setMoney(s, S.fx);
    window.AM_THEME.apply(s);
    const mode = s.mode === 'floating' ? 'floating' : 'taskbar';
    const scale = Number(s.scale) || 1;
    root.style.transform = scale === 1 ? '' : `scale(${scale})`;
    root.innerHTML = R.widget({
      usage: S.usage,
      settings: s,
      system,
      mode,
      // a strip on the desktop keeps a fixed two-row height
      taskbarHeight: (s.mode === 'floating' || docked(s) ? taskbarHeight : Number(s.stripHeight) || 40) / scale,
      docked: docked(s),
    });
    // a new meme bubble (Korean UI): one short sound, and a re-render once it has faded out
    if (R.takeMemeEvents()) {
      api.memeSound().catch(() => {});
      clearTimeout(memeTimer);
      memeTimer = setTimeout(render, R.MEME_MS + 100);
    }
    // reactions and encounters move on a clock: come back for each stage and for the end
    // (also for a random encounter that started during this render)
    clearTimeout(chaosTimer);
    const nextStage = R.fxNext();
    if (nextStage) chaosTimer = setTimeout(render, nextStage + 30);
    const el = root.firstElementChild;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const w = Math.ceil(r.width);
    const h = Math.ceil(r.height);
    const sig = `${mode}:${w}x${h}`;
    if (sig !== lastSize) {
      lastSize = sig;
      api.widgetResize(w, h);
    }
  }

  // drag to move (docked: along the taskbar), double-click → dashboard, right-click → menu
  root.addEventListener('pointerdown', (e) => {
    if (e.button === 0) dragged = false;
    if (e.button !== 0 || e.target.closest('button') || S.settings.lockPosition) return;
    const sx = e.screenX;
    const sy = e.screenY;
    let moved = false;
    root.setPointerCapture(e.pointerId);
    const move = (ev) => {
      if (!moved && Math.hypot(ev.screenX - sx, ev.screenY - sy) > 4) {
        moved = true;
        dragged = true;
        dragging = true;
        api.dragStart(sx, sy);
      }
      if (moved) api.dragMove(ev.screenX, ev.screenY);
    };
    const up = () => {
      root.removeEventListener('pointermove', move);
      root.removeEventListener('pointerup', up);
      root.removeEventListener('pointercancel', up);
      if (moved) {
        dragging = false;
        api.dragEnd();
      }
    };
    root.addEventListener('pointermove', move);
    root.addEventListener('pointerup', up);
    root.addEventListener('pointercancel', up);
  });
  root.addEventListener('dblclick', (e) => {
    if (!e.target.closest('button')) api.openDashboard('overview');
  });
  root.addEventListener('click', (e) => {
    const b = e.target.closest('[data-act]');
    if (b) {
      if (b.dataset.act === 'refresh') api.refresh();
      if (b.dataset.act === 'period') api.setSettings({ statsPeriod: S.settings.statsPeriod === 'week' ? 'today' : 'week' });
      if (b.dataset.act === 'dashboard') api.openDashboard('overview');
      if (b.dataset.act === 'settings') api.openDashboard('display');
      return;
    }
    // a click (not the end of a drag) on an agent in the card: a little mood. The strip stays calm.
    const sec = dragged ? null : e.target.closest('.prov[data-id]');
    dragged = false;
    if (!sec || S.settings.mode !== 'floating' || !R.poke(sec.dataset.id, S.settings)) return;
    render();
    clearTimeout(moodTimer);
    moodTimer = setTimeout(render, R.MOOD_MS + 60);
  });
  document.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    api.widgetMenu();
  });

  api.on('usage', (u) => {
    S.usage = u;
    render();
  });
  api.on('settings', (s) => {
    S.settings = s;
    render();
  });
  api.on('system', (v) => {
    system = v;
    const o = S.settings.mode === 'floating' ? S.settings.floating : S.settings.taskbar;
    if (o && o.system) render();
  });
  api.on('event', (ev) => {
    if (ev && ev.type === 'fx') {
      S.fx = ev.fx;
      render();
    }
    if (ev && ev.type === 'taskbar' && ev.height) {
      taskbarHeight = ev.height;
      lastSize = '';
      render();
    }
  });

  render();
  setInterval(render, 30 * 1000);
})();
