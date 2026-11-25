/* "새 버전이 나왔어요": the new version's patch notes with 지금 업데이트 / 나중에.
 * Opened by the updater (src-tauri/src/updater.rs); the notes are the release body. */
(async function () {
  'use strict';
  const api = window.agentMeter;
  const U = window.AM_UTIL;
  const $ = (s) => document.querySelector(s);
  const esc = U.esc;
  const I18N = window.AM_I18N;
  const tr = (k, v) => I18N.t(k, v);
  const MAX_ITEMS = 5;

  const S = await api.init();
  function applySettings(s) {
    I18N.setLang(s.language);
    I18N.applyDom();
    document.title = tr('uw.title');
    window.AM_THEME.apply(s);
  }
  applySettings(S.settings);
  let u = S.update || {};

  function render() {
    const current = u.current || S.version || '';
    $('#ver').innerHTML = `${current ? `<span>v${esc(current)}</span><span class="arrow">→</span>` : ''}<b>v${esc(u.version || '')}</b>`;
    const items = U.noteItems(u.notes);
    const shown = items.slice(0, MAX_ITEMS).map((t) => `<li>${esc(t)}</li>`);
    if (!items.length) shown.push(`<li>${esc(tr('notes.fallback'))}</li>`);
    if (items.length > MAX_ITEMS) shown.push(`<li class="more">${esc(tr('uw.more', { n: items.length - MAX_ITEMS }))}</li>`);
    $('#notes').innerHTML = shown.join('');

    const busy = u.state === 'downloading' || u.state === 'installing';
    const st = $('#status');
    st.hidden = !(busy || u.state === 'error');
    st.classList.toggle('err', u.state === 'error');
    $('#bar').style.width = `${u.state === 'installing' ? 100 : u.progress || 0}%`;
    $('#status-text').textContent =
      u.state === 'installing'
        ? tr('uw.installing')
        : u.state === 'downloading'
          ? tr('uw.downloading', { p: u.progress || 0 })
          : u.state === 'error'
            ? tr('uw.failed', { e: u.error || '' })
            : '';
    $('#btn-install').disabled = busy;
    $('#btn-install').textContent = u.state === 'error' ? tr('uw.retry') : tr('uw.install');
    $('#btn-later').hidden = busy;
  }

  document.addEventListener('click', (e) => {
    const b = e.target.closest('[data-act]');
    if (!b || b.disabled) return;
    switch (b.dataset.act) {
      case 'install':
        u = { ...u, state: 'downloading', progress: 0 };
        render();
        api.updateInstall();
        break;
      case 'later':
        api.updateLater();
        break;
      case 'all':
        api.openDashboard('notes');
        break;
    }
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && u.state !== 'downloading' && u.state !== 'installing') api.updateLater();
  });

  api.on('event', (ev) => {
    if (ev.type === 'update' && ev.update) {
      // a "checking" blip from a background check must not blank the window
      if (ev.update.version || ev.update.state === 'error') u = { ...u, ...ev.update };
      render();
    }
  });
  api.on('settings', (s) => {
    applySettings(s);
    render();
  });

  render();
})();
