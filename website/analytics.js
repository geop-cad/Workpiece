// Anonymous usage statistics for workpiece.dev: the home page, the example scripts, the 404 page,
// the book (docs/book/statistics.js loads this file) and the browser app (its page loads
// ../analytics.js). PostHog's EU cloud in cookieless mode: nothing is stored on the visitor's
// device, no session replay, no autocapture, no profiles. Only the events below are sent, and none
// carries anything the visitor typed.
//
// - `$pageview`, once per page load (PostHog's own: the URL, the referrer, browser, OS, screen).
// - `section_viewed` {section}: a section of the home page was on screen long enough to be read
//   (once per section per page load).
// - `example_opened` {name, how}: an example was opened from the gallery, the examples page, a
//   script page or the sheet viewer; `how` is app (the part in the app), drawing (its drawing tab
//   in the app) or script.
// - `drawing_opened` {name, sheet, how}: an example's drawing sheet was looked at: `how` is
//   full_size (clicked to see it large), thumbnail (another sheet put in the large place) or pdf.
// - `link_clicked` {target, section, platform?}: a link was followed; `target` is app, docs,
//   github, download (with the platform's label), pypi, privacy, anchor or other.
// - whatever the app sends through `window.workpieceTrack(event, properties)` (its list is in the
//   privacy page): event names of lower-case letters and underscores, properties of short strings,
//   numbers and booleans only — anything else is dropped here.
//
// Off unless the deploy substituted the PostHog key below (website/assemble.sh does, from the
// repository variable POSTHOG_KEY, and only together with the bundle it serves from this site),
// and off for anyone who sends Do Not Track / Global Privacy Control or switched it off on
// privacy.html. The privacy page describes exactly this: change the two together.

(function () {
  "use strict";
  var KEY = "__POSTHOG_KEY__";
  var OPT_OUT_KEY = "workpiece-statistics-off";
  // How long a section must stay in view to count as read.
  var DWELL_MS = 1500;
  // Where this file is served from: the bundle sits beside it, wherever the page is.
  var self = document.currentScript && document.currentScript.src;
  var ROOT = new URL(".", self || location.origin + "/");

  function optedOut() {
    if (navigator.doNotTrack === "1" || window.doNotTrack === "1" || navigator.globalPrivacyControl === true) return true;
    try {
      return localStorage.getItem(OPT_OUT_KEY) === "1";
    } catch (e) {
      return false;
    }
  }

  var on = KEY.indexOf("__") !== 0 && !optedOut();
  var queue = [];
  var send = null; // set once PostHog has loaded

  // The app's (and this file's) one way to send an event. A no-op while statistics are off.
  function clean(properties) {
    var out = {};
    if (!properties || typeof properties !== "object") return out;
    Object.keys(properties).slice(0, 12).forEach(function (k) {
      var v = properties[k];
      if (!/^[a-z_]{1,32}$/.test(k)) return;
      if (typeof v === "string") out[k] = v.slice(0, 80);
      else if (typeof v === "boolean" || (typeof v === "number" && isFinite(v))) out[k] = v;
    });
    return out;
  }
  function track(event, properties, options) {
    if (!on || typeof event !== "string" || !/^[a-z_]{1,40}$/.test(event)) return;
    if (send) send(event, clean(properties), options);
    else if (queue.length < 50) queue.push([event, clean(properties), options]);
  }
  window.workpieceTrack = track;
  if (!on) return;

  function start() {
    var posthog = window.posthog;
    if (!posthog) return;
    posthog.init(KEY, {
      api_host: "https://eu.i.posthog.com",
      cookieless_mode: "always",
      persistence: "memory",
      person_profiles: "never",
      autocapture: false,
      capture_pageview: true,
      capture_pageleave: false,
      disable_session_recording: true,
      capture_heatmaps: false,
      capture_dead_clicks: false,
      capture_exceptions: false,
      capture_performance: false,
      disable_surveys: true,
      advanced_disable_flags: true,
      disable_external_dependency_loading: true,
    });
    send = function (event, properties, options) {
      posthog.capture(event, properties, options);
    };
    queue.splice(0).forEach(function (q) {
      send(q[0], q[1], q[2]);
    });
    watch();
  }

  function sectionOf(el) {
    if (el.closest("header")) return "nav";
    if (el.closest("footer")) return "footer";
    if (el.closest("dialog")) return "sheet";
    var section = el.closest("section");
    if (section) return section.id || section.classList[0] || "unknown";
    return document.querySelector("main section") ? "unknown" : "page";
  }

  function watch() {
    // ── sections read (the home page) ───────────────────────────────────
    var timers = {};
    var seen = {};
    if (window.IntersectionObserver) {
      var sections = new IntersectionObserver(
        function (entries) {
          entries.forEach(function (entry) {
            var name = sectionOf(entry.target);
            if (seen[name]) return;
            // In view: most of the section, or (for one taller than the screen) most of the screen.
            var screen = entry.rootBounds ? entry.intersectionRect.height / entry.rootBounds.height : 0;
            var inView = entry.isIntersecting && (entry.intersectionRatio >= 0.6 || screen >= 0.5);
            if (inView && !timers[name]) {
              timers[name] = setTimeout(function () {
                seen[name] = true;
                track("section_viewed", { section: name });
              }, DWELL_MS);
            } else if (!inView && timers[name]) {
              clearTimeout(timers[name]);
              delete timers[name];
            }
          });
        },
        { threshold: [0, 0.25, 0.5, 0.6, 0.75, 1] },
      );
      document.querySelectorAll("main section").forEach(function (s) {
        sections.observe(s);
      });
    }

    // ── examples opened and links followed ──────────────────────────────
    function exampleOf(el) {
      var holder = el.closest("[data-example]");
      return holder ? holder.getAttribute("data-example") : null;
    }
    function targetOf(a) {
      var url = new URL(a.href, location.href);
      if (a.hasAttribute("data-download")) return { target: "download", platform: a.getAttribute("data-download") };
      if (url.origin === location.origin) {
        var path = url.pathname.slice(ROOT.pathname.length - 1);
        if (path.indexOf("/app/") === 0) return { target: "app" };
        if (path.indexOf("/docs/") === 0) return { target: "docs" };
        if (/privacy(\.html)?$/.test(path)) return { target: "privacy" };
        if (url.hash && url.pathname === location.pathname) return { target: "anchor", anchor: url.hash.slice(1) };
        return { target: "other" };
      }
      if (url.hostname === "github.com") return { target: "github" };
      if (url.hostname === "pypi.org") return { target: "pypi" };
      return { target: "other" };
    }
    function how(a) {
      if (!a) return null;
      var href = a.getAttribute("href") || "";
      if (/scripts\//.test(href) || /\.py$/.test(href)) return "script";
      if (/tab=drawing/.test(href)) return "drawing";
      if (/app\//.test(href)) return "app";
      return null;
    }
    document.addEventListener(
      "click",
      function (e) {
        var el = e.target instanceof Element ? e.target : null;
        if (!el) return;
        var a = el.closest("a[href]");
        var name = exampleOf(el);
        // Sent at once rather than batched: most of these leave the page.
        var sheet = el.closest(".sheet-open, .sheet-thumb");
        var pdf = a && (a.id === "lb-pdf" || a.hasAttribute("data-sheet-name")) ? a : null;
        if (name && (sheet || pdf)) {
          var from = sheet || pdf;
          var key = from.getAttribute("data-sheet-name") || "";
          if (from.id === "lb-pdf") key = (/([^/]+)\.pdf$/.exec(from.getAttribute("href") || "") || ["", ""])[1];
          var kind = sheet ? (sheet.classList.contains("sheet-thumb") ? "thumbnail" : "full_size") : "pdf";
          track("drawing_opened", { name: name, sheet: key, how: kind }, { send_instantly: true });
        } else {
          var way = name && how(a);
          if (way) track("example_opened", { name: name, how: way }, { send_instantly: true });
        }
        if (a) {
          var t = targetOf(a);
          t.section = sectionOf(a);
          track("link_clicked", t, { send_instantly: true });
        }
      },
      { capture: true },
    );
  }

  var script = document.createElement("script");
  script.src = new URL("vendor/posthog/array.no-external.js", ROOT).href;
  script.onload = start;
  document.head.appendChild(script);
})();
