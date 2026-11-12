// SPDX-License-Identifier: GPL-2.0-only
// LibreRAM frontend: talks to the Rust backend through Tauri commands.
const invoke = window.__TAURI__.core.invoke;

const $ = (id) => document.getElementById(id);
const table = $("disks").querySelector("tbody");
let selected = null;
let saving = false;

function setStatus(text) { $("status").textContent = text; }

function selectionButtons() {
  const enabled = Boolean(selected) && !saving;
  for (const id of ["btn-open", "btn-props", "btn-label", "btn-save", "btn-remove"]) {
    $(id).disabled = !enabled;
  }
}

function selectRow(row, disk) {
  table.querySelectorAll(".selected").forEach((r) => {
    r.classList.remove("selected");
    r.setAttribute("aria-checked", "false");
  });
  row.classList.add("selected");
  row.setAttribute("aria-checked", "true");
  selected = disk;
  selectionButtons();
}

// Shared message/confirm/prompt layer: themed <dialog> instead of blocking alert().
function message(title, text, buttons = [{ label: "OK", value: true, primary: true }], input = null, rows = null) {
  return new Promise((resolve) => {
    const dialog = $("msg-dialog");
    $("msg-title").textContent = title;
    const textNode = $("msg-text");
    const rowsNode = $("msg-rows");
    textNode.hidden = !text;
    textNode.textContent = text || "";
    rowsNode.replaceChildren();
    rowsNode.hidden = !rows;
    if (rows) {
      for (const [key, value] of rows) {
        const dt = document.createElement("dt");
        dt.textContent = key;
        const dd = document.createElement("dd");
        dd.textContent = value;
        rowsNode.append(dt, dd);
      }
    }
    const field = $("msg-input");
    if (input) {
      field.hidden = false;
      field.value = input.value || "";
      field.placeholder = input.placeholder || "";
    } else {
      field.hidden = true;
      field.value = "";
    }
    const actions = $("msg-actions");
    actions.replaceChildren();
    for (const spec of buttons) {
      const button = document.createElement("button");
      button.className = "btn" + (spec.primary ? " primary" : "");
      button.textContent = spec.label;
      button.addEventListener("click", () => {
        dialog.close(input ? field.value.trim() : spec.value);
      });
      actions.appendChild(button);
    }
    dialog.addEventListener("close", () => resolve(dialog.returnValue), { once: true });
    dialog.showModal();
    (input ? field : actions.querySelector("button")).focus();
  });
}

const showMessage = (title, text) => message(title, text);
const showRows = (title, rows) => message(title, null, undefined, null, rows);
const showConfirm = (title, text) => message(title, text, [
  { label: "Отмена", value: null },
  { label: "OK", value: true, primary: true },
]);

async function refresh() {
  try {
    const [disks, memory] = await Promise.all([invoke("list_disks"), invoke("memory_info")]);
    const keepUnit = selected ? selected.unit : null;
    table.replaceChildren();
    for (const disk of disks) {
      const row = document.createElement("tr");
      row.dataset.unit = disk.unit;
      row.tabIndex = 0;
      row.setAttribute("role", "radio");
      row.setAttribute("aria-checked", "false");
      const cells = [
        [disk.letter, false],
        [disk.size_text, false],
        [disk.filesystem, true],
        [disk.usage, true],
        [disk.kind, true],
      ];
      for (const [index, [value, dim]] of cells.entries()) {
        const td = document.createElement("td");
        td.textContent = value;
        if (dim) td.className = "dim";
        if (index === 3) {
          // Usage cell doubles as the fill indicator.
          td.classList.add("usage");
          td.style.setProperty("--p", (Math.min(100, parseInt(value) || 0)) / 100);
        }
        row.appendChild(td);
      }
      row.addEventListener("click", () => selectRow(row, disk));
      row.addEventListener("dblclick", () => { selectRow(row, disk); properties(); });
      row.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          selectRow(row, disk);
        } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          const rows = [...table.children];
          const next = rows[rows.indexOf(row) + (event.key === "ArrowDown" ? 1 : -1)];
          if (next) { next.focus(); next.click(); }
        }
      });
      table.appendChild(row);
      if (disk.unit === keepUnit) selectRow(row, disk);
    }
    $("table-empty").hidden = disks.length > 0;
    selected = null;
    selectionButtons();
    const memoryText = memory ? `  ·  RAM: ${(memory.avail_bytes / 2 ** 30).toFixed(1)} из ${(memory.total_bytes / 2 ** 30).toFixed(1)} ГиБ свободно` : "";
    $("summary").textContent = `Устройств: ${disks.length}${memoryText}`;
    $("hint").textContent = disks.length ? "Выберите диск, чтобы отключить его." : "Нет подключённых устройств ImDisk. Создайте первый диск.";
    setStatus("Данные RAM-диска теряются после отключения или выключения компьютера.");
  } catch (error) {
    $("summary").textContent = "Не удалось получить список";
    setStatus(String(error));
  }
}

async function openDisk() {
  const disk = selected;
  if (!disk) return;
  if (disk.letter === "—") return showMessage("Открытие диска", "У диска нет буквы.");
  try {
    await invoke("open_path", { path: disk.letter + "\\" });
  } catch (error) {
    showMessage("Ошибка открытия диска", String(error));
  }
}

async function properties() {
  const disk = selected;
  if (!disk) return;
  try {
    const info = await invoke("disk_properties", { unit: disk.unit });
    showRows(`Свойства · ${info.letter}`, [
      ["Буква", info.letter],
      ["Размер", `${(info.size / 1024 ** 2).toLocaleString("ru-RU", { maximumFractionDigits: 1 })} МиБ`],
      ["Файловая система", info.filesystem],
      ["Кластер", info.cluster],
      ["Занято", info.usage],
      ["Автозапуск ImDisk", info.persistent ? "да" : "нет"],
      ["Устройство", info.description],
    ]);
  } catch (error) {
    showMessage("Ошибка", String(error));
    refresh();
  }
}

function changeLabel() {
  const disk = selected;
  if (!disk) return;
  if (disk.letter === "—") return showMessage("Метка тома", "У диска нет буквы.");
  $("l-value").value = "";
  $("label-form").dataset.letter = disk.letter;
  $("label-dialog").showModal();
}

async function saveImage() {
  const disk = selected;
  if (!disk || saving) return;
  if (disk.letter === "—") return showMessage("Сохранение образа", "Для сохранения нужна буква диска.");
  const path = await message(
    "Сохранить образ",
    "Том будет заблокирован на время копирования; файлы на диске нужно закрыть.",
    [
      { label: "Отмена", value: null },
      { label: "Сохранить", value: "ok", primary: true },
    ],
    { value: "", placeholder: "D:\\backup\\ram.img" },
  );
  if (!path) return;
  saving = true;
  selectionButtons();
  setStatus("Сохранение образа…");
  try {
    await invoke("save_image", { letter: disk.letter, size: disk.size, destination: path });
    setStatus("Образ сохранён: " + path);
  } catch (error) {
    setStatus("Не удалось сохранить образ.");
    showMessage("Ошибка сохранения", String(error));
  } finally {
    saving = false;
    selectionButtons();
  }
}

async function removeDisk() {
  const disk = selected;
  if (!disk || saving) return;
  let removePersistent = false;
  if (disk.persistent) {
    const answer = await message(
      "Отключение диска",
      `${disk.letter}: несохранённые данные будут потеряны. Удалить также настройку автозапуска?`,
      [
        { label: "Отмена", value: null },
        { label: "Удалить диск", value: false },
        { label: "Удалить и автозапуск", value: true, primary: true },
      ],
    );
    if (answer === null || answer === undefined || answer === "") return;
    removePersistent = answer === true;
  } else {
    const answer = await showConfirm("Отключение диска", `Удалить устройство ImDisk ${disk.unit} (${disk.letter})? Несохранённые данные будут потеряны.`);
    if (!answer) return;
  }
  try {
    await invoke("remove_disk", { unit: disk.unit, removePersistent });
  } catch (error) {
    showMessage("Ошибка", String(error));
  }
  refresh();
}

function fillLetters(select, extra) {
  invoke("free_letters").then((letters) => {
    const all = [...new Set([...letters, ...(extra ? [extra] : [])])].sort();
    select.replaceChildren(...all.map((letter) => new Option(letter, letter)));
    if (extra) select.value = extra;
  });
}

function createDialog() {
  fillLetters($("c-letter"));
  invoke("memory_info").then((memory) => {
    $("create-memory").textContent = memory
      ? `Свободно RAM: ${(memory.avail_bytes / 2 ** 20).toLocaleString("ru-RU", { maximumFractionDigits: 0 })} МиБ. Укажите параметры для создания диска`
      : "Укажите параметры для создания диска";
  });
  $("create-dialog").showModal();
}

async function submitCreate(event) {
  event.preventDefault();
  if ($("c-auto").checked) {
    const saved = await invoke("session_load");
    if ((saved || []).some((d) => d.letter === $("c-letter").value)) {
      const overwrite = await message(
        "Диск при входе",
        `Настройки для диска ${$("c-letter").value} уже сохранены. Перезаписать их новыми?`,
        [
          { label: "Отмена", value: null },
          { label: "Перезаписать", value: true, primary: true },
        ],
      );
      if (!overwrite) return;
    }
  }
  try {
    await invoke("create_disk", {
      request: {
        letter: $("c-letter").value,
        sizeMib: Number($("c-size").value),
        filesystem: $("c-fs").value,
        image: $("c-image").value.trim() || null,
        persistent: $("c-persistent").checked,
        allocationSize: $("c-cluster").value === "Авто" ? 0 : Number($("c-cluster").value),
      },
      folders: $("c-folders").value,
      label: $("c-label").value.trim(),
      sessionAuto: $("c-auto").checked,
    });
    $("create-dialog").close();
    setStatus(`Диск ${$("c-letter").value} создан.`);
    refresh();
  } catch (error) {
    showMessage("Ошибка создания диска", String(error));
    refresh();
  }
}

async function sessionDialog() {
  if (document.querySelector("dialog[open]")) return;
  const disks = await invoke("session_load");
  const list = $("s-list");
  list.replaceChildren();
  if (disks && disks.length) {
    for (const disk of disks) addSessionRow(disk);
  } else {
    showMessage("Параметры автозапуска", "Сохранённых параметров пока нет. Создайте диск и включите запуск при входе.");
    createDialog();
    return;
  }
  $("session-dialog").showModal();
}

// One fieldset per logon disk, cloned from the template.
async function addSessionRow(disk) {
  const row = $("s-row-template").content.firstElementChild.cloneNode(true);
  const legend = row.querySelector("legend");
  const select = row.querySelector(".s-letter");
  const states = await invoke("letter_states");
  const options = states.map((s) => {
    const option = new Option(s.letter, s.letter);
    option.disabled = s.blocked && s.letter !== disk?.letter;
    return option;
  });
  select.replaceChildren(...options);
  if (disk) {
    select.value = disk.letter;
    row.querySelector(".s-size").value = disk.size_mib;
    row.querySelector(".s-fs").value = disk.filesystem;
    row.querySelector(".s-cluster").value = disk.allocation_size ? String(disk.allocation_size) : "Авто";
    row.querySelector(".s-label").value = disk.label || "";
    row.querySelector(".s-folders").value = (disk.folders || []).join(", ");
  } else {
    const firstFree = options.find((o) => !o.disabled);
    if (firstFree) select.value = firstFree.value;
    row.querySelector(".s-size").value = 1024;
  }
  const retitle = () => { legend.textContent = `Диск ${select.value || ""}`; };
  select.addEventListener("change", retitle);
  retitle();
  // Saved disks start collapsed; a brand-new one opens expanded, on top.
  if (disk) row.classList.add("collapsed");
  // Collapse to the legend bar; click the bar to expand again.
  legend.addEventListener("click", () => row.classList.toggle("collapsed"));
  row.querySelector(".s-remove").addEventListener("click", () => {
    row.remove();
    if (!$("s-list").children.length) addSessionRow(null);
  });
  if (disk) {
    $("s-list").appendChild(row);
  } else {
    $("s-list").prepend(row);
    row.scrollIntoView({ block: "nearest" });
  }
}

$("s-add").addEventListener("click", () => addSessionRow(null));

async function submitSession(event) {
  event.preventDefault();
  const disks = [...$("s-list").children].map((row) => ({
    letter: row.querySelector(".s-letter").value,
    sizeMib: Number(row.querySelector(".s-size").value),
    filesystem: row.querySelector(".s-fs").value,
    allocationSize: row.querySelector(".s-cluster").value === "Авто" ? 0 : Number(row.querySelector(".s-cluster").value),
    label: row.querySelector(".s-label").value.trim(),
    folders: row.querySelector(".s-folders").value.split(",").map((s) => s.trim()).filter(Boolean),
  }));
  const letters = disks.map((d) => d.letter);
  if (new Set(letters).size !== letters.length) {
    showMessage("Параметры автозапуска", "Буквы дисков повторяются. Используйте разные буквы.");
    return;
  }
  try {
    await invoke("session_enable", { disks });
    $("session-dialog").close();
    setStatus("Параметры сохранены и применятся при следующем входе.");
  } catch (error) {
    showMessage("Параметры автозапуска", String(error));
  }
}

async function submitLabel(event) {
  event.preventDefault();
  try {
    await invoke("change_label", { letter: $("label-form").dataset.letter, label: $("l-value").value.trim() });
    $("label-dialog").close();
    refresh();
    setStatus("Метка тома изменена. Сохранённые параметры диска при входе меняются отдельно.");
  } catch (error) {
    showMessage("Метка тома", String(error));
  }
}

async function ensureImDisk() {
  if (await invoke("imdisk_present")) return;
  if (!(await showConfirm("ImDisk не найден", "Не найден imdisk.exe. Установить драйвер ImDisk сейчас?"))) return;
  setStatus("Установка ImDisk…");
  try {
    await invoke("install_imdisk");
    setStatus("ImDisk установлен.");
  } catch (error) {
    showMessage("Установка ImDisk", String(error));
  }
  refresh();
}

$("btn-refresh").addEventListener("click", refresh);
$("btn-create").addEventListener("click", createDialog);
$("btn-open").addEventListener("click", openDisk);
$("btn-props").addEventListener("click", properties);
$("btn-label").addEventListener("click", changeLabel);
$("btn-save").addEventListener("click", saveImage);
$("btn-remove").addEventListener("click", removeDisk);
$("create-form").addEventListener("submit", submitCreate);
$("c-cancel").addEventListener("click", () => $("create-dialog").close());
$("session-form").addEventListener("submit", submitSession);
$("s-cancel").addEventListener("click", () => $("session-dialog").close());
$("label-form").addEventListener("submit", submitLabel);
$("l-cancel").addEventListener("click", () => $("label-dialog").close());

// Settings menu entry, minimal for parity: Ctrl+, opens the session dialog.
document.addEventListener("keydown", (event) => {
  if (event.key === "," && (event.ctrlKey || event.metaKey)) sessionDialog();
  if (document.querySelector("dialog[open]")) return;
  if (event.key === "F5") { event.preventDefault(); refresh(); }
  if (event.key === "Delete" && selected) removeDisk();
});

window.__TAURI__.event.listen("session-error", (event) => showMessage("Создание диска при входе", String(event.payload)));

ensureImDisk().then(refresh).then(checkUpdate);

async function checkUpdate(manual = false) {
  try {
    const info = await invoke("check_update");
    if (!info) {
      if (manual) showMessage("Обновления", "У вас последняя версия LibreRAM.");
      return;
    }
    const install = await message(
      "Доступна новая версия",
      `LibreRAM ${info.version} доступна для установки. Приложение закроется и запустит установщик; настройки и диски не пострадают.`,
      [
        { label: "Позже", value: null },
        { label: "Скачать и установить", value: true, primary: true },
      ],
    );
    if (!install) return;
    setStatus("Загрузка обновления…");
    await invoke("install_update", { url: info.msi_url });
    setStatus("Установщик запущен.");
  } catch (error) {
    if (manual) showMessage("Обновления", String(error));
    // silent on startup: no network / rate limit must not nag
  }
}

$("tab-settings").addEventListener("click", () => sessionDialog());
$("tab-help").addEventListener("click", async () => {
  const check = await message(
    "LibreRAM 0.3.15 (Tauri)",
    "Управляйте RAM-дисками через ImDisk.\n\n" +
    "Клик по строке — выбор диска; двойной клик — свойства.\n" +
    "Клавиатура: Tab к таблице, стрелки и Enter — выбор, Ctrl+, — параметры входа.\n" +
    "F5 — обновить список, Delete — отключить выбранный диск.\n\n" +
    "Проверить обновления сейчас?",
    [
      { label: "Закрыть", value: null },
      { label: "Проверить обновления", value: true, primary: true },
    ],
  );
  if (check) checkUpdate(true);
});

// Pseudo-element ✕ can't take listeners; route corner clicks to cancel.
for (const dialog of document.querySelectorAll("dialog")) {
  dialog.addEventListener("click", (event) => {
    const box = dialog.getBoundingClientRect();
    const inCorner = event.clientX > box.right - 44 && event.clientY < box.top + 44;
    if (inCorner && event.target === dialog) {
      dialog.close(dialog.id === "msg-dialog" ? null : "");
    }
  });
}
