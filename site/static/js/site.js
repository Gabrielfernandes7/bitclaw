/* BitClaw — site: tema, menu, busca, copiar, índice da página. Sem dependências. */
(function () {
  "use strict";
  var doc = document, root = doc.documentElement;

  function store(key, val) {
    try { if (val === undefined) return localStorage.getItem(key); localStorage.setItem(key, val); } catch (e) {}
    return null;
  }

  // ---- tema
  var themeBtn = doc.querySelector("[data-theme-toggle]");
  if (themeBtn) themeBtn.addEventListener("click", function () {
    var dark = root.dataset.theme === "dark" ||
      (!root.dataset.theme && matchMedia("(prefers-color-scheme: dark)").matches);
    root.dataset.theme = dark ? "light" : "dark";
    store("bc-theme", root.dataset.theme);
  });

  // ---- menu lateral (celular)
  var burger = doc.querySelector("[data-burger]");
  if (burger) {
    burger.addEventListener("click", function () {
      var open = doc.body.classList.toggle("menu-open");
      burger.setAttribute("aria-expanded", open);
    });
    doc.addEventListener("click", function (e) {
      if (doc.body.classList.contains("menu-open") && !e.target.closest(".side") && !e.target.closest("[data-burger]"))
        doc.body.classList.remove("menu-open");
    });
  }

  // ---- copiar comando e blocos de código
  function copy(text, btn, label) {
    function done() { var old = btn.textContent; btn.textContent = "copiado"; setTimeout(function () { btn.textContent = old; }, 1400); }
    if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(done, function () {});
    else { var t = doc.createElement("textarea"); t.value = text; doc.body.appendChild(t); t.select(); try { doc.execCommand("copy"); done(); } catch (e) {} t.remove(); }
  }
  doc.querySelectorAll("[data-copy]").forEach(function (b) {
    b.addEventListener("click", function () { copy(b.getAttribute("data-copy"), b); });
  });
  doc.querySelectorAll(".prose .hl").forEach(function (block) {
    var b = doc.createElement("button");
    b.className = "code-copy"; b.type = "button"; b.textContent = "copiar";
    b.setAttribute("aria-label", "Copiar o bloco de código");
    b.addEventListener("click", function () { copy(block.querySelector("pre").innerText.replace(/\n$/, ""), b); });
    block.appendChild(b);
  });

  // ---- índice da página: destaca a seção visível
  var links = [].slice.call(doc.querySelectorAll(".toc a"));
  if (links.length && "IntersectionObserver" in window) {
    var map = {};
    links.forEach(function (a) { map[a.getAttribute("href").slice(1)] = a; });
    var obs = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) {
        if (en.isIntersecting) {
          links.forEach(function (a) { a.classList.remove("on"); });
          var a = map[en.target.id]; if (a) a.classList.add("on");
        }
      });
    }, { rootMargin: "-70px 0px -70% 0px" });
    links.forEach(function (a) {
      var h = doc.getElementById(a.getAttribute("href").slice(1)); if (h) obs.observe(h);
    });
  }

  // ---- busca (índice gerado no build: search-index.json)
  var dlg = doc.getElementById("search");
  if (!dlg || typeof dlg.showModal !== "function") return;
  var input = dlg.querySelector("[data-search-input]");
  var list = dlg.querySelector("[data-search-results]");
  var prefix = dlg.getAttribute("data-root") || "";
  var index = null, sel = 0, shown = [];

  function fold(s) { return s.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, ""); }
  function esc(s) { return s.replace(/[&<>"]/g, function (c) { return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]; }); }
  function load() {
    if (index) return Promise.resolve(index);
    return fetch(prefix + "search-index.json").then(function (r) { return r.json(); })
      .then(function (d) { index = d; return d; }).catch(function () { index = []; return index; });
  }
  function open() { dlg.showModal(); input.value = ""; render(""); input.focus(); load().then(function () { render(input.value); }); }
  function snippet(page, terms) {
    var text = page.d || "", f = fold(page.x), at = -1;
    for (var i = 0; i < terms.length && at < 0; i++) at = f.indexOf(terms[i]);
    if (at >= 0) {
      // o texto dobrado tem o mesmo comprimento do original na maioria dos casos; só aproximamos
      var from = Math.max(0, at - 40);
      text = (from ? "… " : "") + page.o.slice(from, from + 150) + " …";
    }
    return text;
  }
  function render(q) {
    var terms = fold(q).split(/\s+/).filter(Boolean);
    var data = index || [];
    var res = data.map(function (p) {
      if (!terms.length) return { p: p, s: 1 };
      var title = fold(p.t), heads = fold(p.h.join(" ")), body = p.x, s = 0;
      for (var i = 0; i < terms.length; i++) {
        var t = terms[i], hit = 0;
        if (title.indexOf(t) >= 0) hit += 10;
        if (heads.indexOf(t) >= 0) hit += 4;
        if (body.indexOf(t) >= 0) hit += 1;
        if (!hit) return { p: p, s: 0 };
        s += hit;
      }
      return { p: p, s: s };
    }).filter(function (r) { return r.s > 0; });
    if (terms.length) res.sort(function (a, b) { return b.s - a.s; });
    shown = res.slice(0, 8); sel = 0;
    if (!shown.length) { list.innerHTML = '<li class="empty">Nenhum resultado' + (index ? "." : " (carregando…)") + "</li>"; return; }
    list.innerHTML = shown.map(function (r, i) {
      return '<li><a href="' + prefix + r.p.u + '"' + (i === 0 ? ' class="sel"' : "") + "><small>" + esc(r.p.s) + "</small><strong>" +
        esc(r.p.t) + "</strong><span>" + esc(terms.length ? snippet(r.p, terms) : r.p.d) + "</span></a></li>";
    }).join("");
  }
  function move(d) {
    var as = list.querySelectorAll("a"); if (!as.length) return;
    as[sel].classList.remove("sel"); sel = (sel + d + as.length) % as.length;
    as[sel].classList.add("sel"); as[sel].scrollIntoView({ block: "nearest" });
  }
  input.addEventListener("input", function () { render(input.value); });
  input.addEventListener("keydown", function (e) {
    if (e.key === "ArrowDown") { e.preventDefault(); move(1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); move(-1); }
    else if (e.key === "Enter") { var a = list.querySelectorAll("a")[sel]; if (a) location.href = a.href; }
  });
  dlg.addEventListener("click", function (e) { if (e.target === dlg) dlg.close(); });
  doc.querySelectorAll("[data-search-open]").forEach(function (b) { b.addEventListener("click", open); });
  doc.addEventListener("keydown", function (e) {
    var typing = /input|textarea|select/i.test((e.target.tagName || "")) || e.target.isContentEditable;
    if ((e.key === "/" && !typing) || ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k")) { e.preventDefault(); open(); }
  });
})();
