import { detectLanguage, setLanguage, applyTranslations, t } from "/i18n.js";

const { listen } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;
const { getCurrentWindow, LogicalSize } = window.__TAURI__.window;

// 창 높이를 컨텐츠 실제 높이에 맞춰 계속 맞춰준다 -> 스크롤바 자체가 필요 없어짐.
function resizeToContent() {
  const container = document.querySelector(".container");
  if (!container) return;
  const height = Math.ceil(container.scrollHeight);
  getCurrentWindow().setSize(new LogicalSize(320, height));
}

const FRAME_INTERVAL_MS = { Idle: 700, Normal: 350, High: 130 };
const STAGE_KEY = { Idle: "stageIdle", Normal: "stageNormal", High: "stageHigh" };
// 이미지 수정할 때마다 이 값 올려서 웹뷰 캐시 무효화 (그림 바꿨는데 안 바뀌어 보이면 이거 확인)
const ASSET_VERSION = 23;

// 자루 옆 휴식 6프레임(idle, 0~5) + 먹기 시퀀스 18프레임(eating, 6~23, 6번은 눕기<->앉기 전환 겸용).
const STORY_FRAMES = Array.from(
  { length: 24 },
  (_, i) => `/assets/skin_pixel/story${i + 1}.png?v=${ASSET_VERSION}`
);
const IDLE_END = 5;
const TRANSITION = 6;
const EAT_START = 7;
const EAT_END = 23;
const NETWORK_HISTORY_LEN = 40;

function fmtGB(v) {
  return `${v.toFixed(1)} GB`;
}

function fmtUptime(lang, secs) {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const day = t(lang, "day");
  const hour = t(lang, "hour");
  const minute = t(lang, "minute");
  if (d > 0) return `${d}${day} ${h}${hour} ${m}${minute}`;
  if (h > 0) return `${h}${hour} ${m}${minute}`;
  return `${m}${minute}`;
}

// percent 높을수록 위험(초록→주황→빨강). inverse=true면 반대(배터리처럼 낮을수록 위험).
function levelColor(percent, inverse = false) {
  const p = inverse ? 100 - percent : percent;
  if (p < 50) return "#3fb950";
  if (p < 80) return "#e0a72c";
  return "#e0522c";
}

function setupDropdown(dropdownId, onSelect) {
  const root = document.querySelector(`#${dropdownId}`);
  const toggle = root.querySelector(".dropdown-toggle");
  const items = Array.from(root.querySelectorAll(".dropdown-item"));

  function close() {
    root.classList.remove("dropdown-open");
  }

  function setValue(value, { silent = false } = {}) {
    root.dataset.value = value;
    const active = items.find((el) => el.dataset.value === value);
    if (active) toggle.textContent = active.textContent;
    items.forEach((el) => el.classList.toggle("dropdown-item-active", el.dataset.value === value));
    if (!silent) onSelect(value);
  }

  toggle.addEventListener("click", (e) => {
    e.stopPropagation();
    document.querySelectorAll(".dropdown.dropdown-open").forEach((el) => {
      if (el !== root) el.classList.remove("dropdown-open");
    });
    root.classList.toggle("dropdown-open");
  });

  items.forEach((el) => {
    el.addEventListener("click", () => {
      setValue(el.dataset.value);
      close();
    });
  });

  document.addEventListener("click", close);

  return { setValue, refreshLabel: () => setValue(root.dataset.value, { silent: true }) };
}

window.addEventListener("DOMContentLoaded", () => {
  const resizeObserver = new ResizeObserver(() => resizeToContent());
  resizeObserver.observe(document.querySelector(".container"));

  let currentLang = detectLanguage();
  applyTranslations(currentLang);

  const langDropdown = setupDropdown("lang-dropdown", (value) => {
    currentLang = value;
    setLanguage(currentLang);
    renderCarrotCount(lastCarrotCount);
  });
  langDropdown.setValue(currentLang, { silent: true });

  const capyEl = document.querySelector("#capybara");
  const stageEl = document.querySelector("#capy-stage");

  const cpuPercentEl = document.querySelector("#cpu-percent");
  const cpuDetailEl = document.querySelector("#cpu-detail");
  const cpuBarEl = document.querySelector("#cpu-bar");

  const memPercentEl = document.querySelector("#mem-percent");
  const memDetailEl = document.querySelector("#mem-detail");
  const memBarEl = document.querySelector("#mem-bar");

  const diskPercentEl = document.querySelector("#disk-percent");
  const diskDetailEl = document.querySelector("#disk-detail");
  const diskBarEl = document.querySelector("#disk-bar");

  const batteryPercentEl = document.querySelector("#battery-percent");
  const batteryDetailEl = document.querySelector("#battery-detail");
  const batteryBarEl = document.querySelector("#battery-bar");

  const networkIfaceEl = document.querySelector("#network-iface");
  const networkDetailEl = document.querySelector("#network-detail");
  const networkChartEl = document.querySelector("#network-chart");
  const networkCtx = networkChartEl.getContext("2d");
  const uptimeDetailEl = document.querySelector("#uptime-detail");
  const carrotCountEl = document.querySelector("#carrot-count");

  document.querySelector("#quit-btn").addEventListener("click", () => {
    invoke("quit_app");
  });

  // 당근 개수가 끝없이 늘어나면 숫자가 창 밖으로 넘칠 수 있어서, 한 자루(SACK_SIZE)를
  // 채우면 자루 개수 + 남은 개수로 나눠 표시하고 개수는 0부터 다시 센다.
  const SACK_SIZE = 10_000_000;

  let lastStatus = null;
  let lastCarrotCount = 0;
  function renderCarrotCount(count) {
    lastCarrotCount = count;
    const unit = t(currentLang, "carrotUnit");
    const sackUnit = t(currentLang, "carrotSackUnit");
    const sacks = Math.floor(count / SACK_SIZE);
    const remainder = count % SACK_SIZE;
    const remainderText = unit ? `${remainder}${unit}` : `${remainder}`;
    carrotCountEl.textContent =
      sacks > 0 && sackUnit ? `${sacks}${sackUnit} ${remainderText}` : remainderText;
  }

  invoke("get_carrot_count").then(renderCarrotCount);
  listen("carrot-count", (event) => renderCarrotCount(event.payload));

  let timer = null;
  let index = 0;
  let direction = 1;
  let phase = "idle"; // "idle" | "risingUp" | "eating" | "lyingDown"
  let currentStage = null;

  function animate(stage) {
    if (stage === currentStage) return;
    const previousStage = currentStage;
    currentStage = stage;
    if (timer) clearInterval(timer);

    // 빠른 속도(High)로 막 올라온 순간엔 먹던 당근을 버리고 처음부터 다시
    // 줍는다 — 진행 중이던 사이클은 카운트되지 않는다.
    if (stage === "High" && previousStage !== "High" && phase === "eating") {
      index = EAT_START;
      direction = 1;
    }

    timer = setInterval(() => {
      const isIdle = stage === "Idle";

      if (phase === "idle") {
        if (!isIdle) {
          phase = "risingUp";
          index = TRANSITION;
        } else {
          index += direction;
          if (index >= IDLE_END) {
            index = IDLE_END;
            direction = -1;
          } else if (index <= 0) {
            index = 0;
            direction = 1;
          }
        }
      } else if (phase === "risingUp") {
        phase = "eating";
        index = EAT_START;
        direction = 1;
      } else if (phase === "eating") {
        if (isIdle) {
          phase = "lyingDown";
          index = TRANSITION;
        } else {
          index += direction;
          if (index >= EAT_END) {
            index = EAT_END;
            direction = -1;
          } else if (index <= EAT_START) {
            index = EAT_START;
            direction = 1;
          }
        }
      } else {
        // lyingDown
        phase = "idle";
        index = 0;
        direction = 1;
      }

      capyEl.src = STORY_FRAMES[index];
    }, FRAME_INTERVAL_MS[stage]);
  }

  const uploadHistory = [];
  const downloadHistory = [];

  function drawNetworkChart() {
    const w = networkChartEl.width;
    const h = networkChartEl.height;
    networkCtx.clearRect(0, 0, w, h);

    const allValues = uploadHistory.concat(downloadHistory);
    const max = Math.max(10, ...allValues);

    function drawLine(history, color) {
      if (history.length < 2) return;
      networkCtx.beginPath();
      networkCtx.strokeStyle = color;
      networkCtx.lineWidth = 1.5;
      history.forEach((v, i) => {
        const x = (i / (NETWORK_HISTORY_LEN - 1)) * w;
        const y = h - (v / max) * h;
        if (i === 0) networkCtx.moveTo(x, y);
        else networkCtx.lineTo(x, y);
      });
      networkCtx.stroke();
    }

    drawLine(uploadHistory, "#e0a72c");
    drawLine(downloadHistory, "#3fb9d9");
  }

  function pushNetworkSample(uploadKbps, downloadKbps) {
    uploadHistory.push(uploadKbps);
    downloadHistory.push(downloadKbps);
    if (uploadHistory.length > NETWORK_HISTORY_LEN) uploadHistory.shift();
    if (downloadHistory.length > NETWORK_HISTORY_LEN) downloadHistory.shift();
    drawNetworkChart();
  }

  function applyStatus(s) {
    lastStatus = s;
    stageEl.textContent = `${t(currentLang, STAGE_KEY[s.stage])} — CPU ${s.cpu_percent.toFixed(0)}%`;
    animate(s.stage);

    cpuPercentEl.textContent = `${s.cpu_percent.toFixed(1)}%`;
    cpuDetailEl.textContent = `${t(currentLang, "idleLabel")}: ${(100 - s.cpu_percent).toFixed(1)}%`;
    cpuBarEl.style.width = `${s.cpu_percent}%`;
    cpuBarEl.style.background = levelColor(s.cpu_percent);

    memPercentEl.textContent = `${s.memory.percent.toFixed(0)}%`;
    memDetailEl.textContent = `${fmtGB(s.memory.used_gb)} / ${fmtGB(s.memory.total_gb)} · Swap ${fmtGB(s.memory.swap_used_gb)}/${fmtGB(s.memory.swap_total_gb)}`;
    memBarEl.style.width = `${s.memory.percent}%`;
    memBarEl.style.background = levelColor(s.memory.percent);

    diskPercentEl.textContent = `${s.storage.percent.toFixed(0)}%`;
    diskDetailEl.textContent = `${fmtGB(s.storage.used_gb)} / ${fmtGB(s.storage.total_gb)}`;
    diskBarEl.style.width = `${s.storage.percent}%`;
    diskBarEl.style.background = levelColor(s.storage.percent);

    if (s.battery) {
      batteryPercentEl.textContent = `${s.battery.percent.toFixed(0)}%`;
      batteryDetailEl.textContent = s.battery.charging
        ? t(currentLang, "charging")
        : t(currentLang, "onPower");
      batteryBarEl.style.width = `${s.battery.percent}%`;
      batteryBarEl.style.background = levelColor(s.battery.percent, true);
    } else {
      batteryPercentEl.textContent = t(currentLang, "noBattery");
      batteryDetailEl.textContent = t(currentLang, "noBatteryInfo");
      batteryBarEl.style.width = "0%";
    }

    networkIfaceEl.textContent = s.network.interface !== "-" ? `(${s.network.interface})` : "";
    networkDetailEl.textContent = `IP: ${s.network.local_ip} · ↑${s.network.upload_kbps.toFixed(0)} KB/s · ↓${s.network.download_kbps.toFixed(0)} KB/s`;
    pushNetworkSample(s.network.upload_kbps, s.network.download_kbps);

    uptimeDetailEl.textContent = `${fmtUptime(currentLang, s.uptime_secs)} · ${s.hostname}`;
  }

  listen("system-status", (event) => {
    applyStatus(event.payload);
  });
});
