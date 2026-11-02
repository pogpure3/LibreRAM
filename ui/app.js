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

async function refresh() {
  try {
    const [disks, memory] = await Promise.all([invoke("list_disks"), invoke("memory_info")]);
    table.replaceChildren();
    for (const disk of disks) {
      const row = document.createElement("tr");
      row.dataset.unit = disk.unit;
      const cells = [
        [disk.letter, false],
        [disk.size_text, false],
        [disk.filesystem, true],
        [disk.usage, true],
        [disk.kind, true],
      ];
      for (const [value, dim] of cells) {
        const td = document.createElement("td");
        td.textContent = value;
        if (dim) td.className = "dim";
        row.appendChild(td);
      }
      row.addEventListener("click", () => {
        table.querySelectorAll(".selected").forEach((r) => r.classList.remove("selected"));
        row.classList.add("selected");
        selected = disk;
        selectionButtons();
      });
      row.addEventListener("dblclick", () => { selected = disk; properties(); });
      table.appendChild(row);
    }
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

function selectedRow() { return selected; }

async function openDisk() {
  const disk = selectedRow();
  if (!disk) return;
  if (disk.letter === "—") return alert("У диска нет буквы.");
  try {
    await invoke("open_path", { path: disk.letter + "\\" });
  } catch (error) {
    alert("Ошибка открытия диска: " + error);
  }
}

async function properties() {
  const disk = selectedRow();
  if (!disk) return;
  try {
    const info = await invoke("disk_properties", { unit: disk.unit });
    alert(
      `Буква: ${info.letter}\nРазмер: ${(info.size / 1024 ** 2).toLocaleString("ru-RU", { maximumFractionDigits: 1 })} МиБ\n` +
      `Файловая система: ${info.filesystem}\nКластер: ${info.cluster}\nЗанято: ${info.usage}\n` +
      `Автозапуск ImDisk: ${info.persistent ? "да" : "нет"}\n\n${info.description}`,
    );
  } catch (error) {
    alert(error);
    refresh();
  }
}

function changeLabel() {
  const disk = selectedRow();
  if (!disk) return;
  if (disk.letter === "—") return alert("У диска нет буквы.");
  $("l-value").value = "";
  $("label-form").dataset.letter = disk.letter;
  $("label-dialog").showModal();
}

async function saveImage() {
  const disk = selectedRow();
  if (!disk || saving) return;
  if (disk.letter === "—") return alert("Для сохранения нужна буква диска.");
  const path = prompt("Полный путь к файлу образа (.img):");
  if (!path) return;
  if (!confirm("Том будет заблокирован на время копирования. Продолжить?")) return;
  saving = true;
  selectionButtons();
  setStatus("Сохранение образа…");
  try {
    await invoke("save_image", { letter: disk.letter, size: disk.size, destination: path });
    setStatus("Образ сохранён: " + path);
  } catch (error) {
    setStatus("Не удалось сохранить образ.");
    alert(error);
  } finally {
    saving = false;
    selectionButtons();
  }
}

async function removeDisk() {
  const disk = selectedRow();
  if (!disk || saving) return;
  let removePersistent = false;
  if (disk.persistentHint) {
    const answer = await confirmDialog(`${disk.letter}? Несохранённые данные будут потеряны.\n\nУдалить также настройку автозапуска?\nOK — удалить; «Отмена» — сохранить.`);
    if (answer === null) return;
    removePersistent = answer;
  } else if (!confirm(`Удалить устройство ImDisk ${disk.unit} (${disk.letter})? Несохранённые данные будут потеряны.`)) {
    return;
  }
  try {
    await invoke("remove_disk", { unit: disk.unit, removePersistent });
  } catch (error) {
    alert(error);
  }
  refresh();
}

async function confirmDialog(text) {
  // Persistent removal has three outcomes; window.confirm only two.
  const remove = confirm(text + "\n\nOK — удалить настройку автозапуска.");
  return remove;
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
    refresh();
  } catch (error) {
    alert(error);
    refresh();
  }
}

async function sessionDialog() {
  const config = await invoke("session_load");
  if (!config) {
    alert("Сохранённых параметров пока нет. Создайте диск и включите запуск при входе.");
    createDialog();
    return;
  }
  fillLetters($("s-letter"), config.letter);
  $("s-size").value = config.size_mib;
  $("s-fs").value = config.filesystem;
  $("s-cluster").value = config.allocation_size ? String(config.allocation_size) : "Авто";
  $("s-label").value = config.label || "";
  $("s-folders").value = (config.folders || []).join(", ");
  $("session-dialog").showModal();
}

async function submitSession(event) {
  event.preventDefault();
  try {
    await invoke("session_enable", {
      config: {
        letter: $("s-letter").value,
        sizeMib: Number($("s-size").value),
        filesystem: $("s-fs").value,
        allocationSize: $("s-cluster").value === "Авто" ? 0 : Number($("s-cluster").value),
        label: $("s-label").value.trim(),
        folders: $("s-folders").value.split(",").map((s) => s.trim()).filter(Boolean),
      },
    });
    $("session-dialog").close();
    setStatus("Параметры сохранены и применятся при следующем входе.");
  } catch (error) {
    alert(error);
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
    alert(error);
  }
}

async function ensureImDisk() {
  if (await invoke("imdisk_present")) return;
  if (!confirm("Не найден imdisk.exe. Установить драйвер ImDisk сейчас?")) return;
  setStatus("Установка ImDisk…");
  try {
    await invoke("install_imdisk");
    setStatus("ImDisk установлен.");
  } catch (error) {
    alert(error);
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
$("create-dialog").addEventListener("close", () => {});
$("c-cancel").addEventListener("click", () => $("create-dialog").close());
$("session-form").addEventListener("submit", submitSession);
$("s-cancel").addEventListener("click", () => $("session-dialog").close());
$("label-form").addEventListener("submit", submitLabel);
$("l-cancel").addEventListener("click", () => $("label-dialog").close());

// Settings menu entry, minimal for parity: Ctrl+, opens the session dialog.
document.addEventListener("keydown", (event) => {
  if (event.key === "," && (event.ctrlKey || event.metaKey)) sessionDialog();
});

window.__TAURI__.event.listen("session-error", (event) => alert("Создание диска при входе: " + event.payload));

ensureImDisk().then(refresh);

$("tab-settings").addEventListener("click", (event) => {
  event.currentTarget.blur();
  sessionDialog();
});
$("tab-help").addEventListener("click", (event) => {
  event.currentTarget.blur();
  alert(
    "LibreRAM 0.3.0 (Tauri)\n\n" +
    "Управляйте RAM-дисками через ImDisk.\n\n" +
    "Двойной клик по строке — свойства диска.\n" +
    "Ctrl+, — параметры диска при входе.\n\n" +
    "Данные RAM-диска теряются при отключении и выключении компьютера.",
  );
});
