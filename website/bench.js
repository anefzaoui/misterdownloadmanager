// Benchmarks section: draws bench/results.json (tools/bench/report.py) with Apache ECharts.
(function () {
  "use strict";
  var root = document.getElementById("bench");
  if (!root || !window.fetch) return;
  var data = null;
  var metric = "transfer_ms";
  var charts = [];

  function css(name) {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  }
  function palette() {
    return {
      ink: css("--ink"),
      ink2: css("--ink-2"),
      ink3: css("--ink-3"),
      surface: css("--surface"),
      line: css("--line"),
      idm: css("--bench-idm"),
      mdm: css("--bench-mdm"),
      startup: css("--bench-startup"),
      transfer: css("--bench-transfer"),
      finish: css("--bench-finish"),
    };
  }
  function fmtMs(v) {
    if (v == null) return "—";
    if (v >= 10000) return (v / 1000).toFixed(1) + " s";
    if (v >= 1000) return (v / 1000).toFixed(2) + " s";
    return (v >= 100 ? v.toFixed(0) : v.toFixed(1)) + " ms";
  }
  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }
  function values(sc, client, m) {
    return data.runs
      .filter(function (r) { return r.scenario === sc.id && r.client === client && !r.warmup && r.ok && r[m] != null; })
      .map(function (r) { return r[m]; });
  }
  function dispose() {
    charts.forEach(function (c) { c.dispose(); });
    charts = [];
  }

  // One small chart per scenario: every run as a dot, the box from Q1 to Q3, the median labelled.
  function drawScenarios() {
    var grid = root.querySelector("[data-bench-grid]");
    grid.innerHTML = "";
    var p = palette();
    data.scenarios.forEach(function (sc) {
      var card = el("article", "bench-card");
      var head = el("header");
      head.appendChild(el("h3", null, sc.title));
      var cmp = el("p", "bench-ratio");
      var a = sc.clients.IDM.stats[metric], b = sc.clients.MDM.stats[metric];
      if (sc.provisional) {
        // Numbers shown, no verdict: the scenario is being measured again.
        card.classList.add("is-provisional");
        cmp.appendChild(el("span", "bench-badge", "Being re-measured"));
      } else if (a && b) {
        var r = a.median / b.median;
        cmp.innerHTML = r >= 1
          ? "<b>" + r.toFixed(2) + "×</b> faster"
          : "<b>" + (1 / r).toFixed(2) + "×</b> slower";
        if (metric === "e2e_ms" && sc.compare) {
          cmp.innerHTML += ' <span title="95% bootstrap interval of the ratio of medians; two-sided Mann–Whitney U">(' +
            sc.compare.ratio_ci95[0].toFixed(2) + "–" + sc.compare.ratio_ci95[1].toFixed(2) +
            ", p=" + sc.compare.mann_whitney_p + ")</span>";
        }
      }
      head.appendChild(cmp);
      card.appendChild(head);
      var box = el("div", "bench-chart");
      card.appendChild(box);
      if (sc.provisional) card.appendChild(el("p", "bench-why", sc.provisional));
      // Requests per run: the median, and the most when it is far above it (refused retries).
      var rq = function (c) {
        var st = sc.clients[c].stats.requests;
        if (!st) return "—";
        var med = Math.round(st.median), most = Math.round(st.max);
        return most > 2 * med ? med + " (up to " + most.toLocaleString("en-US") + ")" : String(med);
      };
      var foot = el("p", "bench-meta", "faults: " + sc.faults + " · n=" + (a ? a.n : 0) +
        " each · requests per run: IDM " + rq("IDM") + ", MDM " + rq("MDM"));
      card.appendChild(foot);
      grid.appendChild(card);

      var clients = ["IDM", "MDM"];
      var stats = clients.map(function (c) { return sc.clients[c].stats[metric]; });
      var chart = echarts.init(box, null, { renderer: "svg" });
      charts.push(chart);
      chart.setOption({
        animation: false,
        grid: { left: 44, right: 64, top: 8, bottom: 26 },
        xAxis: {
          type: "value",
          min: 0,
          axisLabel: { color: p.ink3, fontSize: 11, formatter: function (v) { return fmtMs(v); } },
          splitLine: { lineStyle: { color: p.line } },
        },
        yAxis: {
          type: "category",
          data: clients,
          inverse: true,
          axisLabel: { color: p.ink, fontWeight: 600 },
          axisLine: { lineStyle: { color: p.line } },
          axisTick: { show: false },
        },
        tooltip: {
          trigger: "item",
          formatter: function (it) {
            if (it.seriesType === "scatter") return it.name + " run: " + fmtMs(it.value[0]);
            var s = stats[it.dataIndex];
            return s ? clients[it.dataIndex] + "<br>median " + fmtMs(s.median) + "<br>IQR " + fmtMs(s.q1) + "–" +
              fmtMs(s.q3) + "<br>mean " + fmtMs(s.mean) + " ± " + fmtMs(s.sd) + "<br>95% CI of median " +
              fmtMs(s.ci95_median[0]) + "–" + fmtMs(s.ci95_median[1]) : "";
          },
        },
        series: [
          {
            type: "custom",
            renderItem: function (params, api) {
              var i = api.value(0);
              var s = stats[i];
              if (!s) return null;
              var y = api.coord([0, i])[1];
              var x1 = api.coord([s.q1, i])[0], x3 = api.coord([s.q3, i])[0], xm = api.coord([s.median, i])[0];
              var h = 18;
              var color = i === 0 ? p.idm : p.mdm;
              return {
                type: "group",
                children: [
                  { type: "rect", shape: { x: x1, y: y - h / 2, width: Math.max(2, x3 - x1), height: h },
                    style: { fill: color, opacity: 0.22 } },
                  { type: "rect", shape: { x: xm - 1.5, y: y - h / 2 - 3, width: 3, height: h + 6 },
                    style: { fill: color } },
                  { type: "text", x: Math.max(xm, x3) + 8, y: y,
                    style: { text: fmtMs(s.median), fill: p.ink, font: "600 12px Cascadia Mono, Consolas, ui-monospace, monospace",
                             verticalAlign: "middle", backgroundColor: p.surface, padding: [1, 3], borderRadius: 3 },
                    z2: 10 },
                ],
              };
            },
            data: [[0], [1]],
            z: 1,
          },
          {
            type: "scatter",
            symbolSize: 6,
            z: 2,
            data: [].concat(
              values(sc, "IDM", metric).map(function (v) { return { name: "IDM", value: [v, 0], itemStyle: { color: p.idm } }; }),
              values(sc, "MDM", metric).map(function (v) { return { name: "MDM", value: [v, 1], itemStyle: { color: p.mdm } }; })
            ),
            itemStyle: { opacity: 0.85 },
          },
        ],
      });
    });
  }

  // Where the time goes: startup, transfer and finish, median of each, per scenario and client.
  function drawPhases() {
    var box = root.querySelector("[data-bench-phases]");
    var p = palette();
    var cats = [];
    var parts = { startup_ms: [], transfer_ms: [], finish_ms: [] };
    data.scenarios.forEach(function (sc) {
      ["IDM", "MDM"].forEach(function (c) {
        cats.push(c + " · " + sc.id);
        Object.keys(parts).forEach(function (m) {
          var s = sc.clients[c].stats[m];
          parts[m].push(s ? Math.max(0, s.median) : 0);
        });
      });
    });
    var chart = echarts.init(box, null, { renderer: "svg" });
    charts.push(chart);
    var names = { startup_ms: "Startup (command → first request)", transfer_ms: "Transfer (first request → last byte)",
                  finish_ms: "Finish (last byte → file ready)" };
    var colors = { startup_ms: p.startup, transfer_ms: p.transfer, finish_ms: p.finish };
    chart.setOption({
      animation: false,
      legend: { top: 0, textStyle: { color: p.ink2 }, itemWidth: 12, itemHeight: 12 },
      grid: { left: 150, right: 30, top: 40, bottom: 30 },
      tooltip: { trigger: "axis", axisPointer: { type: "shadow" },
                 valueFormatter: function (v) { return fmtMs(v); } },
      xAxis: { type: "log", logBase: 10, min: 1, axisLabel: { color: p.ink3, formatter: function (v) { return fmtMs(v); } },
               splitLine: { lineStyle: { color: p.line } }, name: "ms, log scale", nameTextStyle: { color: p.ink3 } },
      yAxis: { type: "category", data: cats, inverse: true,
               axisLabel: { color: p.ink2, fontSize: 11 }, axisLine: { lineStyle: { color: p.line } }, axisTick: { show: false } },
      series: Object.keys(parts).map(function (m) {
        return { name: names[m], type: "bar", stack: "t", data: parts[m], itemStyle: { color: colors[m] }, barWidth: 12 };
      }),
    });
  }

  // CPU time and peak memory of each client during a run (medians).
  function drawResources() {
    var box = root.querySelector("[data-bench-res]");
    var p = palette();
    var ids = data.scenarios.map(function (s) { return s.id; });
    var med = function (c, m) {
      return data.scenarios.map(function (s) { var st = s.clients[c].stats[m]; return st ? st.median : null; });
    };
    var chart = echarts.init(box, null, { renderer: "svg" });
    charts.push(chart);
    chart.setOption({
      animation: false,
      legend: { top: 0, textStyle: { color: p.ink2 } },
      tooltip: { trigger: "axis" },
      grid: [{ left: 60, right: 20, top: 40, height: "32%" }, { left: 60, right: 20, top: "60%", height: "30%" }],
      xAxis: [
        { type: "category", data: ids, gridIndex: 0, axisLabel: { show: false }, axisLine: { lineStyle: { color: p.line } } },
        { type: "category", data: ids, gridIndex: 1, axisLabel: { color: p.ink2, fontSize: 10, rotate: 20 },
          axisLine: { lineStyle: { color: p.line } } },
      ],
      yAxis: [
        { type: "value", gridIndex: 0, name: "CPU ms", nameTextStyle: { color: p.ink3 },
          axisLabel: { color: p.ink3 }, splitLine: { lineStyle: { color: p.line } } },
        { type: "value", gridIndex: 1, name: "peak MB", nameTextStyle: { color: p.ink3 },
          axisLabel: { color: p.ink3 }, splitLine: { lineStyle: { color: p.line } } },
      ],
      series: [
        { name: "IDM", type: "bar", data: med("IDM", "cpu_ms"), itemStyle: { color: p.idm }, xAxisIndex: 0, yAxisIndex: 0 },
        { name: "MDM", type: "bar", data: med("MDM", "cpu_ms"), itemStyle: { color: p.mdm }, xAxisIndex: 0, yAxisIndex: 0 },
        { name: "IDM", type: "bar", data: med("IDM", "peak_ws_mb"), itemStyle: { color: p.idm }, xAxisIndex: 1, yAxisIndex: 1 },
        { name: "MDM", type: "bar", data: med("MDM", "peak_ws_mb"), itemStyle: { color: p.mdm }, xAxisIndex: 1, yAxisIndex: 1 },
      ],
    });
  }

  function drawTable() {
    var tb = root.querySelector("[data-bench-table] tbody");
    tb.innerHTML = "";
    data.scenarios.forEach(function (sc) {
      ["IDM", "MDM"].forEach(function (c, k) {
        var st = sc.clients[c].stats;
        var tr = document.createElement("tr");
        var cells = [
          k === 0 ? sc.title : "",
          c,
          st.e2e_ms ? st.e2e_ms.n : 0,
          st.e2e_ms ? fmtMs(st.e2e_ms.median) + " (" + fmtMs(st.e2e_ms.q1) + "–" + fmtMs(st.e2e_ms.q3) + ")" : "—",
          st.e2e_ms ? fmtMs(st.e2e_ms.ci95_median[0]) + "–" + fmtMs(st.e2e_ms.ci95_median[1]) : "—",
          st.transfer_ms ? fmtMs(st.transfer_ms.median) : "—",
          st.startup_ms ? fmtMs(st.startup_ms.median) : "—",
          st.connections ? st.connections.median : "—",
          st.requests ? st.requests.median : "—",
          st.cpu_ms ? fmtMs(st.cpu_ms.median) : "—",
          st.peak_ws_mb ? st.peak_ws_mb.median + " MB" : "—",
          k === 0 ? (sc.provisional ? "re-measuring" : sc.compare ? sc.compare.mann_whitney_p : "") : "",
        ];
        cells.forEach(function (v, i) {
          var td = document.createElement(i === 0 ? "th" : "td");
          if (i === 0) td.scope = "row";
          td.textContent = v;
          tr.appendChild(td);
        });
        if (sc.provisional) tr.className = "is-provisional";
        if (sc.clients[c].failed) tr.title = sc.clients[c].failed + " run(s) failed and are excluded";
        tb.appendChild(tr);
      });
    });
    var env = data.environment;
    root.querySelector("[data-bench-env]").textContent =
      env.cpu + " · " + env.ram_gb + " GB RAM · " + env.os + " · IDM " + env.idm_version +
      " · MDM " + env.mdm_commit + " · " + env.date.replace("T", " ");
  }

  function drawStatus() {
    var box = root.querySelector("[data-bench-status]");
    if (!box) return;
    box.hidden = data.status !== "preliminary";
    if (!box.hidden) {
      box.textContent = "Preliminary results from " + data.environment.date.slice(0, 10) +
        ". A full re-run is under way; these numbers will be replaced when it finishes.";
    }
  }

  function drawAll() {
    if (!data || !window.echarts) return;
    dispose();
    drawStatus();
    drawScenarios();
    drawPhases();
    drawResources();
    drawTable();
  }

  // Metric switch.
  root.querySelectorAll("[data-metric]").forEach(function (b) {
    b.addEventListener("click", function () {
      metric = b.getAttribute("data-metric");
      root.querySelectorAll("[data-metric]").forEach(function (o) { o.setAttribute("aria-pressed", String(o === b)); });
      drawAll();
    });
  });

  // Redraw on theme or size changes.
  new MutationObserver(drawAll).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  if (window.matchMedia) window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", drawAll);
  var resizeT;
  window.addEventListener("resize", function () {
    clearTimeout(resizeT);
    resizeT = setTimeout(function () { charts.forEach(function (c) { c.resize(); }); }, 120);
  });

  fetch("bench/results.json")
    .then(function (r) { return r.json(); })
    .then(function (j) { data = j; drawAll(); })
    .catch(function () {
      root.querySelector("[data-bench-grid]").textContent = "The benchmark data could not be loaded.";
    });
})();
