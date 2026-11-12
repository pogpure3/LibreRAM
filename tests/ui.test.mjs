import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { chromium } from "playwright";

const appUrl = new URL("../ui/index.html", import.meta.url).href;
let browser;

before(async () => {
  browser = await chromium.launch({ channel: "msedge" });
});
after(async () => browser?.close());

async function openApp(t, options = {}) {
  const page = await browser.newPage();
  t.after(() => page.close());
  page.setDefaultTimeout(5000);
  await page.addInitScript((options) => {
    const disk = {
      unit: 4, letter: "R:", size: 1024 ** 3, size_text: "1024 МиБ",
      filesystem: "NTFS", usage: "0%", kind: "1 ГБ", persistent: !!options.persistent,
    };
    const calls = [];
    const listeners = {};
    window.review = { calls, listeners, update: options.update };
    window.__TAURI__ = {
      core: {
        async invoke(name, args) {
          calls.push({ name, args });
          switch (name) {
            case "imdisk_present": return !options.missingImDisk;
            case "list_disks": return [disk];
            case "memory_info": return { avail_bytes: 8 * 1024 ** 3, total_bytes: 16 * 1024 ** 3 };
            case "free_letters": return ["R:"];
            case "session_load":
              if (options.delayedSession) {
                return new Promise((resolve) => { window.review.finishSessionLoad = () => resolve([]); });
              }
              return [{ letter: "R:" }];
            case "check_update":
              if (window.review.update?.error) throw window.review.update.error;
              return window.review.update ?? null;
            case "remove_disk":
              if (options.busy && !args.force) throw { message: "Том занят.", can_force: true };
              break;
          }
        },
      },
      event: {
        async listen(name, handler) {
          listeners[name] = handler;
        },
      },
    };
  }, options);
  await page.goto(appUrl);
  if (!options.missingImDisk) {
    await page.waitForFunction(() => window.review.calls.some(({ name }) => name === "check_update"));
  }
  return page;
}

function button(page, name) {
  return page.getByRole("button", { name, exact: true });
}

function calls(page, ...commands) {
  return page.evaluate((commands) => window.review.calls.filter(({ name }) => commands.includes(name)), commands);
}

async function closeMessage(page, action) {
  await page.evaluate(() => {
    window.messageClosed = false;
    document.getElementById("msg-dialog").addEventListener("close", () => {
      window.messageClosed = true;
    }, { once: true });
  });
  await action();
  await page.waitForFunction(() => window.messageClosed);
}

async function selectDisk(page) {
  await page.getByText("R:", { exact: true }).click();
}

for (const method of ["Отмена", "Escape", "крестик"]) {
  test(`отключение не выполняется при отмене через ${method}`, async (t) => {
    const page = await openApp(t);
    await selectDisk(page);
    await button(page, "Отключить диск").click();
    await closeMessage(page, async () => {
      if (method === "Escape") {
        await page.keyboard.press("Escape");
      } else if (method === "крестик") {
        const dialog = await page.locator("#msg-dialog").boundingBox();
        await page.mouse.click(dialog.x + dialog.width - 20, dialog.y + 20);
      } else {
        await button(page, "Отмена").click();
      }
    });
    assert.deepEqual(await calls(page, "remove_disk"), []);
  });
}

test("повторное открытие диалога не сохраняет предыдущее подтверждение", async (t) => {
  const page = await openApp(t, { persistent: true });
  await selectDisk(page);
  await button(page, "Отключить диск").click();
  await closeMessage(page, () => button(page, "Удалить и автозапуск").click());
  await selectDisk(page);
  await button(page, "Отключить диск").click();
  await closeMessage(page, () => page.keyboard.press("Escape"));
  assert.deepEqual(await calls(page, "remove_disk"), [
    { name: "remove_disk", args: { unit: 4, removePersistent: true, force: false } },
  ]);
});

for (const approve of [false, true]) {
  test(`занятый том требует отдельного согласия на принудительное отключение (${approve})`, async (t) => {
    const persistent = !approve;
    const page = await openApp(t, { busy: true, persistent });
    await selectDisk(page);
    await button(page, "Отключить диск").click();
    await closeMessage(page, () => button(page, persistent ? "Удалить и автозапуск" : "Отключить").click());
    await page.getByRole("heading", { name: "Принудительное отключение" }).waitFor();
    await closeMessage(page, () => button(page, approve ? "Отключить принудительно" : "Отмена").click());
    const operations = [{ name: "remove_disk", args: { unit: 4, removePersistent: persistent, force: false } }];
    if (approve) operations.push({ name: "remove_disk", args: { unit: 4, removePersistent: persistent, force: true } });
    assert.deepEqual(await calls(page, "remove_disk"), operations);
  });
}

test("отмена сохранения игнорирует введённый путь", async (t) => {
  const page = await openApp(t);
  await selectDisk(page);
  await button(page, "Сохранить образ").click();
  await page.locator("#msg-input").fill("D:/backup/ram.img");
  await closeMessage(page, () => button(page, "Отмена").click());
  assert.deepEqual(await calls(page, "save_image"), []);
});

test("подтверждение сохранения передаёт введённый путь", async (t) => {
  const page = await openApp(t);
  await selectDisk(page);
  await button(page, "Сохранить образ").click();
  await page.locator("#msg-input").fill("D:/backup/ram.img");
  await closeMessage(page, () => button(page, "Сохранить").click());
  assert.deepEqual(await calls(page, "save_image"), [
    { name: "save_image", args: { letter: "R:", size: 1024 ** 3, destination: "D:/backup/ram.img" } },
  ]);
});

test("фоновое сообщение не перехватывает ответ активного диалога", async (t) => {
  const page = await openApp(t);
  await selectDisk(page);
  await button(page, "Сохранить образ").click();
  await page.evaluate(() => { window.review.listeners["session-error"]({ payload: "Ошибка автозапуска" }); });
  await closeMessage(page, () => button(page, "Отмена").click());
  await page.getByRole("heading", { name: "Создание диска при входе" }).waitFor();
  assert.deepEqual(await calls(page, "save_image"), []);
});

test("отмена предложения установки ImDisk не запускает установку", async (t) => {
  const page = await openApp(t, { missingImDisk: true });
  await closeMessage(page, () => button(page, "Отмена").click());
  assert.deepEqual(await calls(page, "install_imdisk"), []);
});

test("отмена замены сохранённого профиля не создаёт диск", async (t) => {
  const page = await openApp(t);
  await button(page, "Создать диск").click();
  await page.waitForFunction(() => document.getElementById("c-letter").value === "R:");
  await page.locator("#create-dialog").getByRole("button", { name: "Создать диск" }).click();
  await closeMessage(page, () => page.locator("#msg-dialog").getByRole("button", { name: "Отмена" }).click());
  assert.deepEqual(await calls(page, "create_disk"), []);
});

test("закрытие формы во время чтения профилей отменяет создание диска", async (t) => {
  const page = await openApp(t, { delayedSession: true });
  await button(page, "Создать диск").click();
  const form = page.locator("#create-dialog");
  await page.waitForFunction(() => document.getElementById("c-letter").value === "R:");
  await form.getByRole("button", { name: "Создать диск" }).click();
  await page.waitForFunction(() => !!window.review.finishSessionLoad);
  await form.getByRole("button", { name: "Отмена" }).click();
  await page.evaluate(() => window.review.finishSessionLoad());
  assert.deepEqual(await calls(page, "create_disk"), []);
});

test("закрытие справки не проверяет обновления повторно", async (t) => {
  const page = await openApp(t);
  await button(page, "Справка").click();
  await closeMessage(page, () => button(page, "Закрыть").click());
  assert.equal((await calls(page, "check_update")).length, 1);
});

test("«Позже» не открывает страницу релизов и не запускает установщик", async (t) => {
  const page = await openApp(t, { update: { version: "0.3.19" } });
  await closeMessage(page, () => button(page, "Позже").click());
  assert.deepEqual(await calls(page, "open_releases", "install_update"), []);
});

test("подтверждение обновления запрашивает только страницу релизов", async (t) => {
  const page = await openApp(t, { update: { version: "0.3.19" } });
  await closeMessage(page, () => button(page, "Открыть страницу релизов").click());
  assert.deepEqual(await calls(page, "open_releases", "install_update"), [{ name: "open_releases", args: undefined }]);
});

for (const outcome of [null, { error: "Нет связи с GitHub" }]) {
  test(`ручная проверка показывает результат (${outcome?.error ?? "обновлений нет"})`, async (t) => {
    const page = await openApp(t);
    await page.evaluate((outcome) => { window.review.update = outcome; }, outcome);
    await button(page, "Справка").click();
    await closeMessage(page, () => button(page, "Проверить обновления").click());
    await page.getByText(outcome?.error ?? "Доступных обновлений нет.", { exact: true }).waitFor();
    assert.equal((await calls(page, "check_update")).length, 2);
  });
}
