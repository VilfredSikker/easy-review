/* ============================================================
   easy-review documentation — shared chrome
   Builds the top bar, sidebar nav, right-hand TOC, prev/next,
   heading anchors, and code copy buttons. No build step.
   ============================================================ */
(function () {
  'use strict';

  var REPO = 'https://github.com/VilfredSikker/easy-review';

  // Ordered navigation model. `file` is relative to /docs/guide/.
  var NAV = [
    { section: 'Getting Started', pages: [
      { file: 'index.html', title: 'Introduction' },
      { file: 'installation.html', title: 'Installation' },
      { file: 'quick-start.html', title: 'Quick Start' }
    ]},
    { section: 'Core Concepts', pages: [
      { file: 'concepts.html', title: 'How er Works' },
      { file: 'diff-modes.html', title: 'Diff Modes' },
      { file: 'reviewing.html', title: 'Reviewing & Navigation' },
      { file: 'comments.html', title: 'Comments & Questions' },
      { file: 'ai-review.html', title: 'AI Review' },
      { file: 'github.html', title: 'GitHub & Pull Requests' },
      { file: 'configuration.html', title: 'Configuration' },
      { file: 'storage.html', title: 'Review Storage' },
      { file: 'skills.html', title: 'Claude Code Skills' }
    ]},
    { section: 'Terminal UI (er)', pages: [
      { file: 'tui.html', title: 'Overview' },
      { file: 'tui-keybindings.html', title: 'Keybinding Reference' }
    ]},
    { section: 'Desktop App', pages: [
      { file: 'desktop.html', title: 'Overview' },
      { file: 'desktop-features.html', title: 'Features in Depth' }
    ]},
    { section: 'Help', pages: [
      { file: 'troubleshooting.html', title: 'Troubleshooting & FAQ' }
    ]}
  ];

  function currentFile() {
    var path = location.pathname.split('/').pop();
    return path === '' ? 'index.html' : path;
  }

  // Flat ordered list for prev/next.
  function flatPages() {
    var out = [];
    NAV.forEach(function (s) { s.pages.forEach(function (p) { out.push(p); }); });
    return out;
  }

  function el(tag, cls, html) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (html != null) e.innerHTML = html;
    return e;
  }

  function buildTopbar() {
    var bar = el('header', 'topbar');
    bar.innerHTML =
      '<button class="menu-btn" aria-label="Toggle navigation">☰</button>' +
      '<a class="brand" href="index.html">' +
        '<span class="er">er</span>' +
        '<span class="sub">easy-review</span>' +
        '<span class="docs-tag">docs</span>' +
      '</a>' +
      '<div class="links">' +
        '<a class="hide-sm" href="../index.html">Home</a>' +
        '<a href="' + REPO + '" target="_blank" rel="noopener">GitHub ↗</a>' +
      '</div>';
    bar.querySelector('.menu-btn').addEventListener('click', function () {
      document.body.classList.toggle('nav-open');
    });
    return bar;
  }

  function buildSidebar() {
    var aside = el('aside', 'sidebar');
    var cur = currentFile();
    NAV.forEach(function (s) {
      var sec = el('div', 'nav-section');
      sec.appendChild(el('div', 'nav-title', s.section));
      s.pages.forEach(function (p) {
        var a = el('a', 'nav-link', p.title);
        a.href = p.file;
        if (p.file === cur) a.classList.add('active');
        sec.appendChild(a);
      });
      aside.appendChild(sec);
    });
    // Close mobile drawer when a link is chosen.
    aside.addEventListener('click', function (e) {
      if (e.target.classList.contains('nav-link')) document.body.classList.remove('nav-open');
    });
    return aside;
  }

  function slugify(text) {
    return text.toLowerCase().replace(/[^\w]+/g, '-').replace(/^-+|-+$/g, '');
  }

  function decorateHeadings(content) {
    var heads = content.querySelectorAll('h2, h3');
    heads.forEach(function (h) {
      if (!h.id) h.id = slugify(h.textContent);
      var a = el('a', 'anchor', '#');
      a.href = '#' + h.id;
      a.setAttribute('aria-label', 'Link to this section');
      h.appendChild(a);
    });
    return heads;
  }

  function buildToc(heads) {
    if (!heads.length) return null;
    var nav = el('nav', 'toc');
    nav.appendChild(el('div', 'toc-title', 'On this page'));
    heads.forEach(function (h) {
      var a = el('a', h.tagName.toLowerCase(), h.firstChild ? h.childNodes[0].textContent : h.textContent);
      a.href = '#' + h.id;
      nav.appendChild(a);
    });
    return nav;
  }

  function buildPageNav(content) {
    var pages = flatPages();
    var cur = currentFile();
    var idx = pages.findIndex(function (p) { return p.file === cur; });
    if (idx < 0) return;
    var wrap = el('nav', 'page-nav');
    if (idx > 0) {
      var prev = pages[idx - 1];
      var a = el('a', 'prev', '<div class="dir">← Previous</div><div class="ttl">' + prev.title + '</div>');
      a.href = prev.file;
      wrap.appendChild(a);
    } else {
      wrap.appendChild(el('span'));
    }
    if (idx < pages.length - 1) {
      var next = pages[idx + 1];
      var b = el('a', 'next', '<div class="dir">Next →</div><div class="ttl">' + next.title + '</div>');
      b.href = next.file;
      wrap.appendChild(b);
    }
    content.appendChild(wrap);
  }

  function addCopyButtons(content) {
    content.querySelectorAll('pre').forEach(function (pre) {
      var btn = el('button', 'copy-btn', 'copy');
      btn.addEventListener('click', function () {
        var code = pre.querySelector('code');
        navigator.clipboard.writeText((code || pre).innerText).then(function () {
          btn.textContent = 'copied';
          setTimeout(function () { btn.textContent = 'copy'; }, 1400);
        });
      });
      pre.appendChild(btn);
    });
  }

  function scrollSpy(heads, toc) {
    if (!toc) return;
    var links = {};
    toc.querySelectorAll('a').forEach(function (a) { links[a.getAttribute('href').slice(1)] = a; });
    var obs = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) {
        if (en.isIntersecting) {
          Object.keys(links).forEach(function (k) { links[k].classList.remove('active'); });
          if (links[en.target.id]) links[en.target.id].classList.add('active');
        }
      });
    }, { rootMargin: '-10% 0px -75% 0px' });
    heads.forEach(function (h) { obs.observe(h); });
  }

  document.addEventListener('DOMContentLoaded', function () {
    var content = document.querySelector('main.content');
    if (!content) return;

    var heads = decorateHeadings(content);
    addCopyButtons(content);
    buildPageNav(content);

    var topbar = buildTopbar();
    var sidebar = buildSidebar();
    var toc = buildToc(heads);

    var layout = el('div', 'layout');
    layout.appendChild(sidebar);
    content.parentNode.removeChild(content);
    layout.appendChild(content);
    if (toc) layout.appendChild(toc);

    document.body.insertBefore(layout, document.body.firstChild);
    document.body.insertBefore(topbar, document.body.firstChild);

    scrollSpy(heads, toc);
  });
})();
