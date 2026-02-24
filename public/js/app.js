(() => {
  const MAILBOX_REGEX = /^[a-z0-9][a-z0-9._+\-]{0,63}$/;
  const POLL_INTERVAL_MS = 5000;

  const state = {
    mailbox: "",
    email: "",
    messages: [],
    currentMessageID: "",
    currentMessage: null,
    activeTab: "text",
    isEditingMailbox: false,
  };

  const elements = {
    topbar: document.querySelector(".topbar"),
    mailboxInput: document.getElementById("mailboxInput"),
    editMailboxButton: document.getElementById("editMailbox"),
    confirmMailboxButton: document.getElementById("confirmMailbox"),
    cancelMailboxButton: document.getElementById("cancelMailbox"),
    randomMailboxButton: document.getElementById("randomMailbox"),
    copyAddressButton: document.getElementById("copyAddress"),
    refreshMessagesButton: document.getElementById("refreshMessages"),
    currentAddress: document.getElementById("currentAddress"),
    messageCount: document.getElementById("messageCount"),
    mailTableBody: document.getElementById("mailTableBody"),
    messageMeta: document.getElementById("messageMeta"),
    textView: document.getElementById("textView"),
    htmlView: document.getElementById("htmlView"),
    jsonView: document.getElementById("jsonView"),
    statusBar: document.getElementById("statusBar"),
    listAPIExample: document.getElementById("apiListExample"),
    detailAPIExample: document.getElementById("apiDetailExample"),
    tabButtons: Array.from(document.querySelectorAll(".tab-btn")),
  };

  let pollTimer = null;

  function boot() {
    syncTopbarOffset();
    window.addEventListener("resize", syncTopbarOffset);
    bindEvents();
    setEditingMode(false);

    const savedMailbox = localStorage.getItem("mailbox");
    if (savedMailbox && MAILBOX_REGEX.test(savedMailbox)) {
      useMailbox(savedMailbox);
      return;
    }
    useMailbox(generateMailbox());
  }

  function bindEvents() {
    elements.editMailboxButton.addEventListener("click", enterMailboxEditMode);
    elements.confirmMailboxButton.addEventListener("click", confirmMailboxEdit);
    elements.cancelMailboxButton.addEventListener("click", cancelMailboxEdit);

    elements.mailboxInput.addEventListener("keydown", (event) => {
      if (event.key === "Enter" && state.isEditingMailbox) {
        confirmMailboxEdit();
      }
      if (event.key === "Escape" && state.isEditingMailbox) {
        cancelMailboxEdit();
      }
    });

    elements.randomMailboxButton.addEventListener("click", () => {
      useMailbox(generateMailbox());
    });

    elements.copyAddressButton.addEventListener("click", async () => {
      if (!state.email) {
        return;
      }
      try {
        await navigator.clipboard.writeText(state.email);
        setStatus(`已复制 ${state.email}`);
      } catch {
        setStatus("复制失败，请手动复制邮箱地址", true);
      }
    });

    elements.refreshMessagesButton.addEventListener("click", () => {
      refreshMessages(false);
    });

    elements.tabButtons.forEach((button) => {
      button.addEventListener("click", () => switchTab(button.dataset.tab));
    });
  }

  function useMailbox(mailbox) {
    state.mailbox = mailbox;
    state.email = `${mailbox}@${window.location.hostname}`;
    state.currentMessageID = "";
    state.currentMessage = null;
    localStorage.setItem("mailbox", mailbox);

    elements.mailboxInput.value = mailbox;
    elements.currentAddress.textContent = state.email;
    setEditingMode(false);
    syncTopbarOffset();

    renderDetail(null);
    renderAPIExamples();
    refreshMessages(false);
    resetPolling();
    setStatus(`已切换到邮箱 ${state.email}`);
  }

  function enterMailboxEditMode() {
    state.isEditingMailbox = true;
    setEditingMode(true);
    elements.mailboxInput.focus();
    elements.mailboxInput.select();
    setStatus("编辑邮箱前缀后，点击 ✓ 确认或 ✕ 取消。");
  }

  function confirmMailboxEdit() {
    const normalized = normalizeMailbox(elements.mailboxInput.value);
    if (!normalized) {
      setStatus("邮箱前缀格式不正确，只允许字母数字和 . _ + -", true);
      return;
    }
    useMailbox(normalized);
  }

  function cancelMailboxEdit() {
    elements.mailboxInput.value = state.mailbox;
    setEditingMode(false);
    syncTopbarOffset();
    setStatus("已取消修改。");
  }

  function setEditingMode(editing) {
    state.isEditingMailbox = editing;
    elements.mailboxInput.readOnly = !editing;
    elements.editMailboxButton.classList.toggle("hidden", editing);
    elements.confirmMailboxButton.classList.toggle("hidden", !editing);
    elements.cancelMailboxButton.classList.toggle("hidden", !editing);
  }

  function syncTopbarOffset() {
    if (!elements.topbar) {
      return;
    }
    const topbarHeight = Math.ceil(elements.topbar.getBoundingClientRect().height);
    document.documentElement.style.setProperty("--topbar-height", `${topbarHeight}px`);
  }

  function renderAPIExamples() {
    const mailboxSegment = encodeURIComponent(state.mailbox || "demo");
    const selectedID = state.currentMessageID || "{id}";
    elements.listAPIExample.textContent = `GET /api/mailboxes/${mailboxSegment}/messages`;
    elements.detailAPIExample.textContent = `GET /api/mailboxes/${mailboxSegment}/messages/${selectedID}`;
  }

  function resetPolling() {
    if (pollTimer) {
      window.clearInterval(pollTimer);
    }
    pollTimer = window.setInterval(() => refreshMessages(true), POLL_INTERVAL_MS);
  }

  async function refreshMessages(isAutoRefresh) {
    if (!state.mailbox) {
      return;
    }

    const endpoint = `/api/mailboxes/${encodeURIComponent(state.mailbox)}/messages`;
    try {
      const response = await fetchJSON(endpoint);
      const messages = Array.isArray(response.messages) ? response.messages : [];
      state.messages = messages;

      elements.messageCount.textContent = `${messages.length} 封`;
      renderTable();

      if (!messages.length) {
        renderDetail(null);
        if (!isAutoRefresh) {
          setStatus("暂无邮件，稍后刷新即可。");
        }
        return;
      }

      const selectedExists = messages.some((item) => item.id === state.currentMessageID);
      if (!selectedExists) {
        await loadMessage(messages[0].id);
      } else if (!isAutoRefresh) {
        await loadMessage(state.currentMessageID);
      }

      if (!isAutoRefresh) {
        setStatus(`已刷新，当前 ${messages.length} 封邮件。`);
      }
    } catch (error) {
      setStatus(error.message || "加载邮件失败", true);
    }
  }

  function renderTable() {
    const tbody = elements.mailTableBody;
    tbody.innerHTML = "";

    if (!state.messages.length) {
      const row = document.createElement("tr");
      const cell = document.createElement("td");
      cell.colSpan = 3;
      cell.textContent = "暂无邮件";
      row.appendChild(cell);
      tbody.appendChild(row);
      return;
    }

    state.messages.forEach((message) => {
      const row = document.createElement("tr");
      row.dataset.id = message.id;
      if (state.currentMessageID === message.id) {
        row.classList.add("active");
      }

      row.innerHTML = `
        <td>${escapeHTML(message.from || "-")}</td>
        <td>${escapeHTML(message.subject || "无主题")}</td>
        <td>${formatDate(message.date || message.received_at)}</td>
      `;

      row.addEventListener("click", () => {
        loadMessage(message.id);
      });

      tbody.appendChild(row);
    });
  }

  async function loadMessage(messageID) {
    if (!state.mailbox || !messageID) {
      return;
    }

    try {
      const endpoint = `/api/mailboxes/${encodeURIComponent(state.mailbox)}/messages/${encodeURIComponent(messageID)}`;
      const response = await fetchJSON(endpoint);
      state.currentMessageID = messageID;
      state.currentMessage = response.message || null;
      renderAPIExamples();
      renderTable();
      renderDetail(state.currentMessage);
    } catch (error) {
      setStatus(error.message || "加载邮件详情失败", true);
    }
  }

  function renderDetail(message) {
    if (!message) {
      elements.messageMeta.textContent = "等等就来( ͡° ͜ʖ ͡°)";
      elements.textView.textContent = "我的邮件在哪里？\n\n等等就来( ͡° ͜ʖ ͡°)";
      elements.htmlView.srcdoc = "";
      elements.jsonView.textContent = "{}";
      switchTab("text");
      return;
    }

    const subject = message.subject || "无主题";
    const from = message.from || "-";
    const date = formatDate(message.date || message.received_at);
    elements.messageMeta.textContent = `${from} · ${date}`;

    const text = message.text || "(无纯文本内容)";
    const html = message.html || "<p style=\"padding:12px;color:#64748b;\">无 HTML 内容</p>";
    elements.textView.textContent = `主题: ${subject}\n发件人: ${from}\n收件人: ${message.to || "-"}\n时间: ${date}\n\n${text}`;
    elements.htmlView.srcdoc = html;
    elements.jsonView.textContent = JSON.stringify(message, null, 2);
    switchTab(state.activeTab);
  }

  function switchTab(tabName) {
    state.activeTab = tabName;

    elements.tabButtons.forEach((button) => {
      const isActive = button.dataset.tab === tabName;
      button.classList.toggle("active", isActive);
    });

    elements.textView.classList.toggle("hidden", tabName !== "text");
    elements.htmlView.classList.toggle("hidden", tabName !== "html");
    elements.jsonView.classList.toggle("hidden", tabName !== "json");
  }

  function normalizeMailbox(input) {
    let value = String(input || "").trim().toLowerCase();
    if (!value) {
      return "";
    }

    if (value.includes("@")) {
      value = value.split("@")[0];
    }
    if (!MAILBOX_REGEX.test(value)) {
      return "";
    }
    return value;
  }

  function generateMailbox() {
    return `mail${Math.random().toString(36).slice(2, 10)}`;
  }

  async function fetchJSON(url) {
    const response = await fetch(url, { cache: "no-store" });
    const payload = await response.json().catch(() => ({}));
    if (!response.ok) {
      throw new Error(payload.error || `请求失败: ${response.status}`);
    }
    return payload;
  }

  function formatDate(value) {
    if (!value) {
      return "-";
    }
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) {
      return "-";
    }
    return date.toLocaleString("zh-CN", { hour12: false });
  }

  function setStatus(message, isError) {
    elements.statusBar.textContent = message;
    elements.statusBar.style.color = isError ? "#c0392b" : "#6b6b6b";
  }

  function escapeHTML(text) {
    return String(text)
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll("\"", "&quot;")
      .replaceAll("'", "&#39;");
  }

  boot();
})();
