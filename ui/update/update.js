/* "새 버전이 나왔어요": the new version's patch notes with 지금 업데이트 / 나중에.
 * Opened by the updater (src-tauri/src/updater.rs); the notes are the release body. */
(async function () {
  'use strict';
  const api = window.agentMeter;
  const U = window.AM_UTIL;
  const $ = (s) => document.querySelector(s);
  const esc = U.esc;
  const MAX_ITEMS = 5;

  const S = await api.init();
  window.AM_THEME.apply(S.settings);
  let u = S.update || {};

  function render() {
    const current = u.current || S.version || '';
    $('#ver').innerHTML = `${current ? `<span>v${esc(current)}</span><span class="arrow">→</span>` : ''}<b>v${esc(u.version || '')}</b>`;
    const items = U.noteItems(u.notes);
    const shown = items.slice(0, MAX_ITEMS).map((t) => `<li>${esc(t)}</li>`);
    if (!items.length) shown.push('<li>버그 수정과 개선</li>');
    if (items.length > MAX_ITEMS) shown.push(`<li class="more">외 ${items.length - MAX_ITEMS}가지 변경</li>`);
    $('#notes').innerHTML = shown.join('');

    const busy = u.state === 'downloading' || u.state === 'installing';
    const st = $('#status');
    st.hidden = !(busy || u.state === 'error');
    st.classList.toggle('err', u.state === 'error');
    $('#bar').style.width = `${u.state === 'installing' ? 100 : u.progress || 0}%`;
    $('#status-text').textContent =
      u.state === 'installing'
        ? '설치하고 다시 시작해요…'
        : u.state === 'downloading'
          ? `내려받는 중 ${u.progress || 0}%`
          : u.state === 'error'
            ? `업데이트하지 못했어요: ${u.error || ''}`
            : '';
    $('#btn-install').disabled = busy;
    $('#btn-install').textContent = u.state === 'error' ? '다시 시도' : '지금 업데이트';
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
  api.on('settings', (s) => window.AM_THEME.apply(s));

  render();
})();
