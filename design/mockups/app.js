// Style mockups: renders the same mock data in three visual styles.
(() => {
  const { groups, entries, recent } = window.MOCK;
  const $ = (s, r = document) => r.querySelector(s);
  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const icon = (n, cls = '') => `<svg class="i ${cls}" aria-hidden="true"><use href="#i-${n}"/></svg>`;
  const keys = (arr) => (arr ? `<span class="keys">${arr.map((k) => `<kbd>${k}</kbd>`).join('')}</span>` : '');

  const STYLES = {
    a: '像 Windows 11 自带的应用：云母背景、Segoe UI、系统强调色，深浅色跟随系统。上手没有门槛，但个性最弱；快捷键只在鼠标悬停和提示里出现。',
    b: '快捷键就是按钮：每个能复制的字段旁边是对应的键帽，点它和按键效果一样，按键时屏幕上的键帽也会按下去。表格式列表、信息密度高，底部常驻快捷键栏。偏向深色。',
    c: '靛蓝染布做底色，缝线做分隔，留白多，列表两行显示，标题用宋体，最有辨识度。宋体（思源宋体）需要随程序打包一个字重，安装包会大 8–10 MB；不打包就退回微软雅黑。',
  };

  const S = {
    style: 'a', theme: 'light', view: 'main',
    group: 'root', sel: 'github', mq: '',
    q: 'acme', qi: 0,
    recent: 0, pw: '', err: '',
    revealed: new Set(), dirty: true,
  };

  // ---------------------------------------------------------------- state <-> URL

  function readHash() {
    const [style, theme, view] = location.hash.slice(1).split('-');
    if (STYLES[style]) S.style = style;
    if (theme === 'light' || theme === 'dark') S.theme = theme;
    if (['main', 'quick', 'unlock'].includes(view)) S.view = view;
  }
  function writeHash() {
    history.replaceState(null, '', `#${S.style}-${S.theme}-${S.view}`);
  }

  // ---------------------------------------------------------------- helpers

  const host = (url) => url.replace(/^[a-z]+:\/\//, '').split('/')[0];
  const groupById = (id) => groups.find((g) => g.id === id);
  function groupPath(id) {
    const g = groupById(id);
    const parts = [groupById('root').name];
    if (g.parent) parts.push(groupById(g.parent).name);
    if (id !== 'root') parts.push(g.name);
    return parts.map(esc).join('<span class="sep"> / </span>');
  }
  const inGroup = (e, g) => (g === 'root' ? e.group !== 'bin' : e.group === g);
  const count = (g) => entries.filter((e) => inGroup(e, g)).length;

  function hue(s) {
    let h = 0;
    for (const ch of s) h = (h * 31 + ch.codePointAt(0)) >>> 0;
    return h % 6;
  }
  function avatar(e, size = '') {
    const ch = [...e.title][0];
    return `<span class="av ${size} h${hue(e.title)}" aria-hidden="true"><span>${esc(/[a-z]/i.test(ch) ? ch.toUpperCase() : ch)}</span></span>`;
  }
  const mask = () => `<span class="mask" aria-label="已隐藏">••••••••••••</span>`;

  function hl(text, q) {
    if (!q) return esc(text);
    const i = text.toLowerCase().indexOf(q.toLowerCase());
    if (i < 0) return esc(text);
    return esc(text.slice(0, i)) + `<mark>${esc(text.slice(i, i + q.length))}</mark>` + esc(text.slice(i + q.length));
  }

  function matches(e, q) {
    const t = q.toLowerCase();
    return [e.title, e.user, e.url, e.notes, ...e.tags].some((v) => v.toLowerCase().includes(t));
  }
  function score(e, q) {
    const t = q.toLowerCase();
    const title = e.title.toLowerCase();
    if (title.startsWith(t)) return 0;
    if (title.includes(t)) return 1;
    if (e.user.toLowerCase().includes(t)) return 2;
    if (e.url.toLowerCase().includes(t)) return 3;
    return 4;
  }
  const byTitle = (a, b) => a.title.localeCompare(b.title, 'zh-Hans-CN');
  const byModified = (a, b) => b.modified.localeCompare(a.modified);

  function visibleEntries() {
    const list = S.mq
      ? entries.filter((e) => e.group !== 'bin' && matches(e, S.mq))
      : entries.filter((e) => inGroup(e, S.group));
    return list.sort(byModified);
  }

  // TOTP: a fake but stable code per entry and 30 s window
  function totp(e) {
    const now = Date.now() / 1000;
    const step = Math.floor(now / 30);
    let h = 2166136261;
    for (const ch of e.id + step) h = Math.imul(h ^ ch.charCodeAt(0), 16777619) >>> 0;
    const code = String(h % 1000000).padStart(6, '0');
    const left = 30 - Math.floor(now % 30);
    return { code: `${code.slice(0, 3)} ${code.slice(3)}`, left, p: left / 30 };
  }
  function totpHtml(e) {
    const t = totp(e);
    return `<span class="totp" data-totp="${e.id}"><span class="code">${t.code}</span><span class="ring" style="--p:${t.p}"></span><span class="secs">${t.left} 秒</span></span>`;
  }

  // ---------------------------------------------------------------- pieces

  function mark() {
    return `<span class="mark" aria-hidden="true">${icon('key')}<b>M</b></span>`;
  }

  function searchBox() {
    return `<label class="search">${icon('search')}<input id="mq" type="text" placeholder="搜索" value="${esc(S.mq)}" autocomplete="off" spellcheck="false" aria-label="搜索条目">${keys(['Ctrl', 'F'])}</label>`;
  }

  function titlebar(main) {
    return `<header class="titlebar">
      <div class="brand">${mark()}<span class="app">Mikoshi</span></div>
      ${main ? `<div class="file"><span class="fname">${esc(window.MOCK.file)}</span>${S.dirty ? '<span class="dirty" title="有未保存的修改（Ctrl+S 保存）"></span>' : ''}</div>` : ''}
      <div class="tb-mid">${main && S.style === 'a' ? searchBox() : ''}</div>
      ${main ? `<button class="tb-btn" data-act="lock" title="锁定 (Ctrl+L)">${icon('lock')}<span class="lbl">锁定</span>${keys(['Ctrl', 'L'])}</button>` : ''}
      <div class="caption" aria-hidden="true"><span>${icon('min')}</span><span>${icon('max')}</span><span class="x">${icon('close')}</span></div>
    </header>`;
  }

  function groupsPane() {
    return `<nav class="groups" aria-label="分组">${groups
      .map((g) => {
        const on = g.id === S.group && !S.mq;
        return `<button class="g d${g.depth}${on ? ' on' : ''}${g.id === 'bin' ? ' bin' : ''}" data-group="${g.id}">${icon(g.icon || 'folder')}<span class="gname">${esc(g.name)}</span><span class="count">${count(g.id)}</span></button>`;
      })
      .join('')}</nav>`;
  }

  function listHead(n) {
    const title = S.mq ? `搜索“${esc(S.mq)}”` : esc(groupById(S.group).name);
    return `<div class="list-head">
      ${S.style !== 'a' ? searchBox() : ''}
      <div class="lh-row"><span class="lh-title">${title}</span><span class="lh-count">${n} 个条目</span>
        <button class="icon-btn" data-act="new" title="新建条目 (Ctrl+N)">${icon('plus')}</button></div>
      <div class="cols" aria-hidden="true"><span></span><span>标题</span><span>用户名</span><span>网址</span><span class="sorted">修改时间${icon('chevron')}</span></div>
    </div>`;
  }

  function row(e) {
    const q = S.mq;
    return `<div class="row${e.id === S.sel ? ' on' : ''}${e.expired ? ' expired' : ''}" data-id="${e.id}" role="option" aria-selected="${e.id === S.sel}">
      ${avatar(e)}
      <span class="t"><span class="tt">${hl(e.title, q)}</span>${e.expired ? '<span class="badge">已过期</span>' : ''}</span>
      <span class="u">${hl(e.user, q)}</span>
      <span class="h">${hl(host(e.url), q)}</span>
      <span class="m">${e.modified.slice(0, 10)}</span>
    </div>`;
  }

  function field(f) {
    const reveal = f.secret
      ? `<button class="act" data-act="reveal" data-key="${f.key}" title="${f.shown ? '隐藏' : '显示'}${f.revealKeys ? ' (Ctrl+H)' : ''}">${icon(f.shown ? 'eye-off' : 'eye')}<span class="lbl">${f.shown ? '隐藏' : '显示'}</span>${keys(f.revealKeys)}</button>`
      : '';
    const open = f.open
      ? `<button class="act" data-act="open" title="打开网址 (Ctrl+Shift+U)">${icon('open')}<span class="lbl">打开</span>${keys(f.open)}</button>`
      : '';
    const copy = `<button class="act copy" data-act="copy" data-what="${f.what}" title="复制${f.copy ? ` (${f.copy.join('+')})` : ''}">${icon('copy')}<span class="lbl">复制</span>${keys(f.copy)}</button>`;
    return `<div class="f${f.cls ? ' ' + f.cls : ''}"><div class="f-label">${f.label}</div><div class="f-value">${f.value}</div><div class="f-acts">${open}${reveal}${copy}</div></div>`;
  }

  function detail(e) {
    const pwShown = S.revealed.has(e.id + ':pw');
    const custom = e.fields
      .map((c, i) => {
        const key = `${e.id}:f${i}`;
        const shown = S.revealed.has(key);
        const value = c.secret ? (shown ? `<span class="secret-plain">${esc(c.v)}</span>` : mask()) : esc(c.v);
        return field({ label: esc(c.k), value, what: 'f' + i, secret: c.secret, shown, key, cls: 'custom' });
      })
      .join('');
    return `<div class="d-scroll">
      <header class="d-head">
        ${avatar(e, 'lg')}
        <div class="d-titles"><h2 class="d-title">${esc(e.title)}</h2><div class="d-path">${groupPath(e.group)}</div></div>
        <div class="d-acts">
          <button class="btn primary" data-act="autotype" title="自动输入 (Ctrl+Shift+V)">${icon('type')}<span class="lbl">自动输入</span>${keys(['Ctrl', 'Shift', 'V'])}</button>
          <button class="btn" data-act="edit" title="编辑 (Ctrl+E)">${icon('edit')}<span class="lbl">编辑</span>${keys(['Ctrl', 'E'])}</button>
        </div>
      </header>
      ${e.expired ? `<div class="expired-note">${icon('warn')}<span>这个条目已于 ${e.expires.slice(0, 10)} 过期，记得去改密码。</span></div>` : ''}
      ${e.tags.length ? `<div class="tags">${e.tags.map((t) => `<span class="tag">${icon('tag')}${esc(t)}</span>`).join('')}</div>` : ''}
      <div class="fields">
        ${field({ label: '用户名', value: esc(e.user), what: 'user', copy: ['Ctrl', 'B'] })}
        ${field({ label: '密码', value: pwShown ? `<span class="secret-plain">${esc(e.pw)}</span>` : mask(), what: 'pw', copy: ['Ctrl', 'C'], secret: true, shown: pwShown, key: e.id + ':pw', revealKeys: ['Ctrl', 'H'] })}
        ${field({ label: '网址', value: `<span class="link">${esc(e.url)}</span>`, what: 'url', copy: ['Ctrl', 'U'], open: ['Ctrl', 'Shift', 'U'] })}
        ${e.totp ? field({ label: '验证码', value: totpHtml(e), what: 'totp', copy: ['Ctrl', 'T'], cls: 'f-totp' }) : ''}
        ${custom}
      </div>
      ${e.notes ? `<section class="d-sec notes"><h3>备注</h3><p>${esc(e.notes)}</p></section>` : ''}
      ${e.files.length ? `<section class="d-sec files"><h3>附件</h3><ul>${e.files.map((f) => `<li>${icon('clip')}<span class="fn">${esc(f.name)}</span><span class="fs">${f.size}</span><button class="act" data-act="export" title="导出到文件">${icon('download')}<span class="lbl">导出</span></button></li>`).join('')}</ul></section>` : ''}
      <footer class="times">
        <div><span>创建</span><span>${e.created}</span></div>
        <div><span>修改</span><span>${e.modified}</span></div>
        <div class="${e.expired ? 'bad' : ''}"><span>过期</span><span>${e.expires}</span></div>
      </footer>
    </div>`;
  }

  function hint(k, label) {
    return `<span class="hint">${keys(k)}<span class="hl">${label}</span></span>`;
  }

  function statusbar(n) {
    return `<footer class="status">
      ${hint(['Ctrl', 'F'], '搜索')}${hint(['Ctrl', 'B'], '用户名')}${hint(['Ctrl', 'C'], '密码')}${hint(['Ctrl', 'T'], '验证码')}${hint(['Ctrl', 'Shift', 'V'], '自动输入')}${hint(['Ctrl', 'L'], '锁定')}
      <span class="st-right">${n} 个条目</span>
    </footer>`;
  }

  // ---------------------------------------------------------------- views

  function mainView() {
    const list = visibleEntries();
    if (!list.some((e) => e.id === S.sel) && list.length) S.sel = list[0].id;
    const sel = list.find((e) => e.id === S.sel);
    return `<div class="win main">
      ${titlebar(true)}
      <div class="body">
        ${groupsPane()}
        <section class="list">${listHead(list.length)}<div class="rows" role="listbox" aria-label="条目">${list.map(row).join('') || `<div class="empty">没有匹配“${esc(S.mq)}”的条目。按 Esc 清除搜索。</div>`}</div></section>
        <section class="detail">${sel ? detail(sel) : '<div class="empty">选择左边的条目查看详情</div>'}</section>
      </div>
      ${statusbar(list.length)}
    </div>`;
  }

  function quickResults() {
    const pool = entries.filter((e) => e.group !== 'bin');
    if (!S.q) return [...pool].sort((a, b) => b.modified.localeCompare(a.modified)).slice(0, 6);
    return pool.filter((e) => matches(e, S.q)).sort((a, b) => score(a, S.q) - score(b, S.q) || byTitle(a, b)).slice(0, 8);
  }

  function qrow(e, i) {
    const q = S.q;
    const t = q.toLowerCase();
    const sub = !q || e.user.toLowerCase().includes(t) || !e.url.toLowerCase().includes(t) ? e.user : host(e.url);
    return `<div class="qr${i === S.qi ? ' on' : ''}" data-qi="${i}" role="option" aria-selected="${i === S.qi}">
      ${avatar(e)}
      <span class="qr-main"><span class="qr-t">${hl(e.title, q)}</span><span class="qr-sub">${hl(sub, q)}</span></span>
      <span class="qr-path">${groupPath(e.group)}</span>
      ${e.totp ? `<span class="qr-otp" title="有验证码">${icon('clock')}</span>` : '<span class="qr-otp"></span>'}
    </div>`;
  }

  function fakeBrowser() {
    return `<div class="fb" aria-hidden="true">
      <div class="fb-tabs"><span class="fb-tab"><span class="fb-fav"></span>登录到 GitLab</span><span class="fb-tab dim">需求评审纪要</span></div>
      <div class="fb-bar"><span class="fb-url">git.acme-corp.cn/users/sign_in</span></div>
      <div class="fb-page"><div class="fb-form">
        <div class="fb-logo"></div><div class="fb-h">登录 GitLab</div>
        <div class="fb-l">用户名或邮箱</div><div class="fb-in focus"><span class="caret"></span></div>
        <div class="fb-l">密码</div><div class="fb-in"></div>
        <div class="fb-btn">登录</div>
      </div></div>
    </div>`;
  }

  function quickView() {
    const res = quickResults();
    if (S.qi >= res.length) S.qi = Math.max(0, res.length - 1);
    return `<div class="desk">
      ${fakeBrowser()}
      <div class="qs" role="dialog" aria-label="快速搜索">
        <label class="qs-input">${icon('search')}<input id="q" value="${esc(S.q)}" placeholder="搜索密码库" autocomplete="off" spellcheck="false" aria-label="搜索密码库"><span class="qs-db">${icon('vault')}个人密码库</span></label>
        ${S.q ? '' : '<div class="qs-cap">最近使用</div>'}
        <div class="qs-results" role="listbox">${res.map(qrow).join('') || `<div class="qs-empty">没有匹配“${esc(S.q)}”的条目。</div>`}</div>
        <footer class="qs-foot">
          ${hint(['Enter'], '复制密码')}${hint(['Ctrl', 'Enter'], '自动输入')}${hint(['Ctrl', 'B'], '复制用户名')}${hint(['Ctrl', 'T'], '复制验证码')}${hint(['Esc'], '关闭')}
        </footer>
      </div>
    </div>`;
  }

  function unlockView() {
    const f = recent[S.recent];
    return `<div class="win unlock">
      ${titlebar(false)}
      <div class="ul">
        <aside class="ul-side">
          <div class="ul-crest" aria-hidden="true">${icon('lock')}</div>
          <div class="ul-brandname">Mikoshi</div>
          <div class="ul-recent">
            <div class="ul-cap">最近打开</div>
            ${recent.map((r, i) => `<button class="ur${i === S.recent ? ' on' : ''}" data-recent="${i}">${icon('vault')}<span class="ur-main"><span class="ur-n">${esc(r.name)}</span><span class="ur-p">${esc(r.path)}</span></span>${keys(['Alt', String(i + 1)])}</button>`).join('')}
            <button class="ur other" data-act="other-file">${icon('folder')}<span class="ur-main"><span class="ur-n">打开其他文件…</span></span>${keys(['Ctrl', 'O'])}</button>
          </div>
        </aside>
        <form class="ul-form" id="ulform" autocomplete="off">
          <div class="ul-file">${icon('vault')}<span class="ulf-main"><span class="ulf-n">${esc(f.name)}</span><span class="ulf-p">${esc(f.path)}</span></span>${icon('chevron', 'ulf-chev')}</div>
          <h1 class="ul-h">解锁密码库</h1>
          <label class="ul-l" for="pw">主密码</label>
          <div class="ul-pw"><input id="pw" type="password" value="${esc(S.pw)}" autocomplete="off"><button type="button" class="act" data-act="ul-reveal" title="显示密码">${icon('eye')}</button></div>
          <button type="button" class="ul-key" data-act="keyfile">${icon('key')}<span>添加密钥文件</span></button>
          <div class="ul-err" role="alert">${S.err ? `${icon('warn')}<span>${S.err}</span>` : ''}</div>
          <button type="submit" class="primary ul-go">解锁${keys(['Enter'])}</button>
        </form>
      </div>
    </div>`;
  }

  // ---------------------------------------------------------------- render

  const mock = $('#mock');
  const screen = $('#screen');
  const frame = $('#frame');

  function render() {
    const active = document.activeElement && document.activeElement.id;
    mock.dataset.style = S.style;
    mock.dataset.theme = S.theme;
    mock.dataset.view = S.view;
    document.body.dataset.theme = S.theme;
    screen.innerHTML = S.view === 'main' ? mainView() : S.view === 'quick' ? quickView() : unlockView();
    for (const b of document.querySelectorAll('[data-style-btn]')) b.setAttribute('aria-selected', b.dataset.styleBtn === S.style);
    for (const b of document.querySelectorAll('[data-view-btn]')) b.setAttribute('aria-selected', b.dataset.viewBtn === S.view);
    $('[data-theme-btn]').textContent = S.theme === 'light' ? '深色' : '浅色';
    $('#desc').textContent = STYLES[S.style];
    if (active && ['mq', 'q', 'pw'].includes(active)) focusInput(active);
    const on = screen.querySelector('.row.on, .qr.on');
    if (on) on.scrollIntoView({ block: 'nearest' });
    writeHash();
  }

  function focusInput(id) {
    const el = document.getElementById(id);
    if (!el) return;
    el.focus();
    const n = el.value.length;
    try { el.setSelectionRange(n, n); } catch { /* password inputs */ }
  }

  function fit() {
    const st = $('.stage');
    const s = Math.min(1, (st.clientWidth - 32) / 1200, (st.clientHeight - 32) / 760);
    frame.style.width = `${1200 * s}px`;
    frame.style.height = `${760 * s}px`;
    mock.style.transform = `scale(${s})`;
  }

  // ---------------------------------------------------------------- feedback

  let toastTimer = 0;
  function toast(msg, clears = false) {
    const t = $('#toast');
    t.innerHTML = `<span class="t-icon">${icon('check')}</span><span class="t-msg">${msg}</span>${clears ? '<span class="t-bar"></span>' : ''}`;
    t.classList.remove('show');
    void t.offsetWidth;
    t.classList.add('show');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => t.classList.remove('show'), clears ? 10000 : 2600);
  }

  function press(what) {
    const btn = screen.querySelector(`.act[data-what="${what}"], .act[data-act="${what}"], .btn[data-act="${what}"], .tb-btn[data-act="${what}"]`);
    if (!btn) return;
    btn.classList.add('pressed');
    setTimeout(() => btn.classList.remove('pressed'), 180);
  }

  // Quick search hides after an action; the mockup brings it back so it can be tried again.
  function away() {
    const qs = screen.querySelector('.qs');
    if (!qs) return;
    qs.classList.add('away');
    setTimeout(() => { qs.classList.remove('away'); focusInput('q'); }, 1600);
  }

  const current = () => entries.find((e) => e.id === S.sel);

  function copy(what) {
    const e = current();
    if (!e) return;
    press(what);
    if (what === 'user') return toast(`已复制 ${esc(e.title)} 的用户名`);
    if (what === 'url') return toast('已复制网址');
    if (what === 'pw') return toast(`已复制 ${esc(e.title)} 的密码，10 秒后清除剪贴板`, true);
    if (what === 'totp') {
      if (!e.totp) return toast(`${esc(e.title)} 没有设置验证码`);
      return toast('已复制验证码，10 秒后清除剪贴板', true);
    }
    const i = Number(what.slice(1));
    const f = e.fields[i];
    if (f) toast(`已复制“${esc(f.k)}”${f.secret ? '，10 秒后清除剪贴板' : ''}`, f.secret);
  }

  function act(name, el) {
    const e = current();
    switch (name) {
      case 'copy': return copy(el.dataset.what);
      case 'reveal': {
        const k = el.dataset.key;
        S.revealed.has(k) ? S.revealed.delete(k) : S.revealed.add(k);
        return render();
      }
      case 'lock': S.view = 'unlock'; S.pw = ''; S.err = ''; S.revealed.clear(); render(); return focusInput('pw');
      case 'autotype': press('autotype'); return toast(`自动输入 ${e ? esc(e.title) : ''}：会先切回上一个窗口再输入（样稿里不执行）`);
      case 'edit': press('edit'); return toast('编辑功能在样稿里不可用');
      case 'new': return toast('新建条目在样稿里不可用');
      case 'open': press('open'); return toast(`已在浏览器中打开 ${e ? esc(host(e.url)) : ''}`);
      case 'export': return toast('附件导出在样稿里不可用');
      case 'keyfile': return toast('会打开文件选择框，选一个密钥文件');
      case 'other-file': return toast('会打开文件选择框');
      case 'ul-reveal': {
        const pw = $('#pw');
        pw.type = pw.type === 'password' ? 'text' : 'password';
        return pw.focus();
      }
      default:
    }
  }

  function move(dir) {
    if (S.view === 'main') {
      const list = visibleEntries();
      const i = list.findIndex((e) => e.id === S.sel);
      const next = list[Math.max(0, Math.min(list.length - 1, i + dir))];
      if (next) { S.sel = next.id; render(); }
    } else if (S.view === 'quick') {
      const n = quickResults().length;
      S.qi = Math.max(0, Math.min(n - 1, S.qi + dir));
      render();
    }
  }

  function unlock() {
    if (!S.pw) {
      S.err = '请输入主密码';
      render();
      return focusInput('pw');
    }
    S.err = '';
    S.pw = '';
    S.view = 'main';
    render();
  }

  // ---------------------------------------------------------------- events

  document.addEventListener('click', (ev) => {
    const t = ev.target.closest('button, [data-id], [data-qi], [data-group]');
    if (!t) return;
    if (t.dataset.styleBtn) { S.style = t.dataset.styleBtn; return render(); }
    if (t.dataset.viewBtn) { S.view = t.dataset.viewBtn; S.err = ''; return render(); }
    if (t.hasAttribute('data-theme-btn')) { S.theme = S.theme === 'light' ? 'dark' : 'light'; return render(); }
    if (t.dataset.group) { S.group = t.dataset.group; S.mq = ''; S.sel = ''; return render(); }
    if (t.dataset.id) { S.sel = t.dataset.id; return render(); }
    if (t.dataset.qi) { S.qi = Number(t.dataset.qi); return render(); }
    if (t.dataset.recent) { S.recent = Number(t.dataset.recent); render(); return focusInput('pw'); }
    if (t.dataset.act) { ev.preventDefault(); return act(t.dataset.act, t); }
  });

  document.addEventListener('dblclick', (ev) => {
    const r = ev.target.closest('.qr');
    if (r) { const e = quickResults()[Number(r.dataset.qi)]; if (e) { toast(`已复制 ${esc(e.title)} 的密码，10 秒后清除剪贴板`, true); away(); } }
  });

  document.addEventListener('input', (ev) => {
    if (ev.target.id === 'mq') { S.mq = ev.target.value.trim(); render(); }
    if (ev.target.id === 'q') { S.q = ev.target.value.trim(); S.qi = 0; render(); }
    if (ev.target.id === 'pw') { S.pw = ev.target.value; if (S.err) { S.err = ''; const err = $('.ul-err'); if (err) err.innerHTML = ''; } }
  });

  document.addEventListener('submit', (ev) => {
    if (ev.target.id === 'ulform') { ev.preventDefault(); unlock(); }
  });

  document.addEventListener('keydown', (ev) => {
    const k = ev.key.length === 1 ? ev.key.toLowerCase() : ev.key;
    const ctrl = ev.ctrlKey || ev.metaKey;
    const shift = ev.shiftKey;
    const inText = document.activeElement && ['INPUT', 'TEXTAREA'].includes(document.activeElement.tagName);

    if (S.view === 'main') {
      if (ctrl && !shift && k === 'f') { ev.preventDefault(); return focusInput('mq'); }
      if (ctrl && !shift && ['b', 'c', 'u', 't'].includes(k)) {
        if (k === 'c' && inText && document.activeElement.selectionStart !== document.activeElement.selectionEnd) return;
        ev.preventDefault();
        return copy({ b: 'user', c: 'pw', u: 'url', t: 'totp' }[k]);
      }
      if (ctrl && shift && k === 'u') { ev.preventDefault(); return act('open'); }
      if (ctrl && shift && k === 'v') { ev.preventDefault(); return act('autotype'); }
      if (ctrl && !shift && k === 'h') { ev.preventDefault(); press('reveal'); return act('reveal', { dataset: { key: S.sel + ':pw' } }); }
      if (ctrl && !shift && k === 'l') { ev.preventDefault(); return act('lock'); }
      if (ctrl && !shift && k === 'e') { ev.preventDefault(); return act('edit'); }
      if (ctrl && !shift && k === 's') { ev.preventDefault(); S.dirty = false; render(); return toast('已保存'); }
      if (k === 'ArrowDown' || k === 'ArrowUp') { ev.preventDefault(); return move(k === 'ArrowDown' ? 1 : -1); }
      if (k === 'Escape' && S.mq) { S.mq = ''; render(); return focusInput('mq'); }
      if (k === 'Enter' && !inText) return act('edit');
    }

    if (S.view === 'quick') {
      if (k === 'ArrowDown' || k === 'ArrowUp') { ev.preventDefault(); return move(k === 'ArrowDown' ? 1 : -1); }
      const e = quickResults()[S.qi];
      const done = (msg, clears) => { ev.preventDefault(); if (!e) return; toast(msg, clears); away(); };
      if (k === 'Enter' && ctrl) return done(`自动输入 ${esc(e?.title ?? '')}：切回“登录到 GitLab”窗口后输入（样稿里不执行）`);
      if (k === 'Enter') return done(`已复制 ${esc(e?.title ?? '')} 的密码，10 秒后清除剪贴板`, true);
      if (ctrl && k === 'b') return done(`已复制 ${esc(e?.title ?? '')} 的用户名`);
      if (ctrl && k === 't') return done(e?.totp ? '已复制验证码，10 秒后清除剪贴板' : `${esc(e?.title ?? '')} 没有设置验证码`, e?.totp);
      if (k === 'Escape') { ev.preventDefault(); return away(); }
      if (!inText && k.length === 1 && !ctrl) focusInput('q');
    }

    if (S.view === 'unlock') {
      if (ev.altKey && ['1', '2', '3'].includes(k)) { ev.preventDefault(); S.recent = Number(k) - 1; render(); return focusInput('pw'); }
      if (ctrl && k === 'o') { ev.preventDefault(); return act('other-file'); }
    }
  });

  setInterval(() => {
    for (const el of screen.querySelectorAll('.totp')) {
      const e = entries.find((x) => x.id === el.dataset.totp);
      const t = totp(e);
      el.querySelector('.code').textContent = t.code;
      el.querySelector('.ring').style.setProperty('--p', t.p);
      el.querySelector('.secs').textContent = `${t.left} 秒`;
    }
  }, 1000);

  window.addEventListener('resize', fit);
  window.addEventListener('hashchange', () => { readHash(); render(); });
  readHash();
  render();
  fit();
  if (S.view === 'quick') focusInput('q');
})();
