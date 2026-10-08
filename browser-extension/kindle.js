(() => {
  if (globalThis.__textHaloReader) return;
  const VERSION = "kindle-dom-0.5";
  const supported = () =>
    ["read.amazon.ca", "read.amazon.com"].includes(location.hostname) &&
    /^[A-Z0-9]{10}$/i.test(
      new URL(location.href).searchParams.get("asin") || "",
    );
  const rectangle = (el) => {
    const r = el.getBoundingClientRect();
    return { x: r.x, y: r.y, width: r.width, height: r.height };
  };
  const visible = (el) => {
    const r = rectangle(el);
    if (
      r.width < 3 ||
      r.height < 3 ||
      r.x >= innerWidth ||
      r.y >= innerHeight ||
      r.x + r.width <= 0 ||
      r.y + r.height <= 0
    )
      return false;
    for (let p = el; p instanceof Element; p = p.parentElement) {
      const s = getComputedStyle(p);
      if (
        s.display === "none" ||
        s.visibility === "hidden" ||
        Number(s.opacity) === 0
      )
        return false;
    }
    return true;
  };
  const name = (el) =>
    (
      el.getAttribute("aria-label") ||
      el.getAttribute("title") ||
      el.textContent ||
      ""
    )
      .trim()
      .replace(/\s+/g, " ");
  const hint = (el) =>
    `${el.tagName.toLowerCase()}${el.id ? "#" + el.id : ""}.${String(
      el.className?.baseVal ?? el.className ?? "",
    )
      .trim()
      .replace(/\s+/g, ".")}`.slice(0, 240);
  const all = (selector) => Array.from(document.querySelectorAll(selector));
  const hash = (text) => {
    let value = 2166136261;
    for (let i = 0; i < text.length; i++)
      value = Math.imul(value ^ text.charCodeAt(i), 16777619);
    return (value >>> 0).toString(16);
  };
  const control = (direction) =>
    all('button,[role="button"],input[type="button"]').find(
      (el) =>
        name(el).toLowerCase() === `${direction} page` &&
        !el.disabled &&
        el.getAttribute("aria-disabled") !== "true",
    );
  function inspect() {
    const base = {
      protocolVersion: 1,
      supported: supported(),
      platform: supported() ? "kindle" : null,
      adapterVersion: VERSION,
      url: location.href,
      bookId: new URL(location.href).searchParams.get("asin"),
      viewport: {
        width: innerWidth,
        height: innerHeight,
        devicePixelRatio,
        visualScale: visualViewport?.scale ?? 1,
      },
      timestamp: Date.now(),
    };
    if (!supported())
      return {
        ...base,
        status: "unsupported",
        reason: "Open a supported Kindle book.",
      };
    const progressCandidates = all("body *")
      .filter(
        (el) =>
          visible(el) &&
          el.textContent.trim().length < 140 &&
          /^(Page|Location)\s*\d+/i.test(el.textContent.trim()),
      )
      .sort((a, b) => rectangle(a).height - rectangle(b).height);
    const progress =
      progressCandidates[0]?.textContent.trim().replace(/\s+/g, " ") ||
      all('[role="slider"],input[type="range"]')
        .map(
          (el) =>
            el.getAttribute("aria-valuetext") ||
            el.getAttribute("aria-label") ||
            "",
        )
        .find((text) => /^(Page|Location)\s*\d+/i.test(text)) ||
      "";
    const titleCandidates = all("h1,h2,[title],[aria-label],div,span").filter(
      (el) =>
        visible(el) &&
        rectangle(el).y < innerHeight * 0.18 &&
        el.childElementCount === 0 &&
        name(el).length > 8 &&
        name(el).length < 250,
    );
    const title =
      titleCandidates
        .find(
          (el) =>
            !/^(Kindle Library|Reader settings|Reader menu|Search|Table of Contents|Bookmark|Page \d)/i.test(
              name(el),
            ),
        )
        ?.textContent?.trim() || "";
    // Render surfaces are discovered rather than guessing fixed viewport margins.
    const surfaces = all("canvas,img,iframe,svg,div").filter((el) => {
      const r = rectangle(el);
      return (
        (el.tagName !== "DIV" ||
          getComputedStyle(el).backgroundImage !== "none") &&
        visible(el) &&
        r.width >= innerWidth * 0.2 &&
        r.height >= innerHeight * 0.2
      );
    });
    const diagnosticElements = surfaces.length
      ? surfaces
      : all("div,iframe,canvas").filter((el) => {
          const r = rectangle(el);
          return (
            visible(el) &&
            r.width > innerWidth * 0.3 &&
            r.height > innerHeight * 0.3 &&
            el.childElementCount <= 2
          );
        });
    const diagnostics = diagnosticElements
      .map((el) => ({
        element: hint(el),
        bounds: rectangle(el),
        type: el.tagName.toLowerCase(),
        source:
          el.tagName === "IFRAME"
            ? (el.getAttribute("src") || "").slice(0, 200)
            : undefined,
      }))
      .slice(0, 20);
    const refusal = (reason) => ({
      ...base,
      status: "needs-confirmation",
      reason,
      title,
      progress,
      diagnostics,
    });
    if (!progress) return refusal("No reliable page/location indicator found.");
    if (!surfaces.length)
      return refusal("No visible page image, canvas, SVG, or frame found.");
    let region = { x: Infinity, y: Infinity, right: 0, bottom: 0 };
    for (const el of surfaces) {
      const r = rectangle(el);
      region.x = Math.min(region.x, r.x);
      region.y = Math.min(region.y, r.y);
      region.right = Math.max(region.right, r.x + r.width);
      region.bottom = Math.max(region.bottom, r.y + r.height);
    }
    region = {
      x: region.x,
      y: region.y,
      width: region.right - region.x,
      height: region.bottom - region.y,
    };
    if (
      region.x < 0 ||
      region.y < 0 ||
      region.x + region.width > innerWidth + 1 ||
      region.y + region.height > innerHeight + 1 ||
      region.width * region.height > innerWidth * innerHeight * 0.93
    )
      return refusal(
        "Page surface is clipped or indistinguishable from the full viewport.",
      );
    const panels = all(
      '[role="dialog"],[role="menu"],[aria-label="menu"],[aria-modal="true"]',
    ).filter(visible);
    if (
      panels.some((el) => {
        const r = rectangle(el);
        return (
          r.x < region.x + region.width &&
          r.x + r.width > region.x &&
          r.y < region.y + region.height &&
          r.y + r.height > region.y
        );
      })
    )
      return refusal("Close the reader menu or dialog.");
    // Verify painted content at several points; sibling menus can cover a still-visible canvas.
    for (const x of [0.2, 0.5, 0.8])
      for (const y of [0.2, 0.5, 0.8]) {
        const stack = document.elementsFromPoint(
          region.x + region.width * x,
          region.y + region.height * y,
        );
        const front = stack.find(
          (el) => getComputedStyle(el).pointerEvents !== "none",
        );
        if (
          front &&
          !surfaces.some(
            (el) => el === front || el.contains(front) || front.contains(el),
          )
        ) {
          const style = getComputedStyle(front);
          // Verified on the live Kindle reader: this transparent sibling is the
          // page-turn hit layer, not a painted panel. Keep all other occluders refused.
          if (
            front.matches("div.pagination-container") &&
            style.backgroundColor === "rgba(0, 0, 0, 0)" &&
            style.backgroundImage === "none" &&
            !front.textContent.trim()
          )
            continue;
          return {
            ...refusal("Another element covers the reading area."),
            occlusion: {
              element: hint(front),
              bounds: rectangle(front),
              role: front.getAttribute("role"),
              label: front.getAttribute("aria-label"),
              background: style.backgroundColor,
              backgroundImage: style.backgroundImage,
              text: front.textContent.trim().slice(0, 120),
            },
          };
        }
      }
    const ranges = all('input[type="range"],[role="slider"]')
      .map((el) => el.getAttribute("aria-valuenow") || el.value || "")
      .join("|");
    const fingerprint = hash(
      progress +
        "|" +
        ranges +
        "|" +
        surfaces
          .map((el) =>
            [
              hint(el),
              el.getAttribute("src"),
              el.getAttribute("data-page"),
              getComputedStyle(el).backgroundImage,
              rectangle(el).width,
              rectangle(el).height,
            ].join(":"),
          )
          .join("|"),
    );
    const navigationDetails = all('button,[role="button"],input[type="button"]')
      .filter((el) => /^(next|previous) page$/i.test(name(el)))
      .map((el) => ({
        element: hint(el),
        parent: el.parentElement ? hint(el.parentElement) : "",
        label: name(el),
        bounds: rectangle(el),
        visible: visible(el),
        disabled: !!el.disabled || el.getAttribute("aria-disabled") === "true",
        role: el.getAttribute("role"),
      }));
    const navigation = { previous: !!control("previous"), next: !!control("next") };
    const position = progress.match(/^(?:Page|Location)\s+(\d+)(?:\s+of\s+(\d+))?/i);
    const current = position ? Number(position[1]) : null;
    const total = position?.[2] ? Number(position[2]) : null;
    // At the cover Kindle removes Previous entirely. Absence alone is not
    // enough: require an edge position and a visible control in the other direction.
    const visibleControl = (label) => navigationDetails.some(
      (c) => c.label.toLowerCase() === label && c.visible && !c.disabled,
    );
    return {
      ...base,
      status: "ready",
      title,
      progress,
      region,
      fingerprint,
      diagnostics,
      navigation,
      navigationDetails,
      startOfBook: navigationDetails.some(
        (c) => c.label.toLowerCase() === "previous page" && c.disabled,
      ) || (current === 1 && !navigation.previous && visibleControl("next page")),
      endOfBook: navigationDetails.some(
        (c) => c.label.toLowerCase() === "next page" && c.disabled,
      ) || (total !== null && total > 0 && current === total && !navigation.next && visibleControl("previous page")),
      text: "",
    };
  }
  // Kindle controls can listen for mouse press/release rather than click alone.
  // Use one activation sequence and let each caller verify its resulting state.
  function activate(element) {
    const point = rectangle(element);
    const mouse = {
      bubbles: true,
      cancelable: true,
      view: window,
      clientX: point.x + point.width / 2,
      clientY: point.y + point.height / 2,
      button: 0,
    };
    element.dispatchEvent(new MouseEvent("mousedown", { ...mouse, buttons: 1 }));
    element.dispatchEvent(new MouseEvent("mouseup", { ...mouse, buttons: 0 }));
    element.click();
  }
  async function navigate(message) {
    const before = inspect();
    if (
      before.status !== "ready" ||
      before.bookId !== message.bookId ||
      before.fingerprint !== message.fingerprint
    )
      throw Error(
        "The reader changed. Inspect it again before turning a page.",
      );
    if (!["next", "previous"].includes(message.direction))
      throw Error("Invalid navigation direction.");
    const button = control(message.direction);
    if (!button) throw Error("The requested page control is unavailable.");
    activate(button);
    const deadline = Date.now() + 7000;
    let last = "",
      stable = 0;
    while (Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 250));
      const after = inspect();
      if (after.bookId !== before.bookId)
        throw Error("The opened book changed.");
      if (
        after.status === "ready" &&
        after.fingerprint !== before.fingerprint
      ) {
        stable = after.fingerprint === last ? stable + 1 : 0;
        last = after.fingerprint;
        if (stable >= 2) return { ...after, navigationVerified: true };
      }
    }
    throw Error(
      "Page control was clicked, but a changed reading position could not be verified.",
    );
  }
  const api = { inspect, navigate };
  globalThis.__textHaloReader = api;
  chrome.runtime.onMessage.addListener((message, sender, respond) => {
    if (sender.id !== chrome.runtime.id || message?.protocolVersion !== 1)
      return;
    if (message.type === "inspect") {
      respond({ ok: true, data: inspect() });
      return;
    }
    if (message.type === "navigate") {
      navigate(message).then(
        (data) => respond({ ok: true, data }),
        (error) => respond({ ok: false, error: error.message }),
      );
      return true;
    }
  });
  // Presence messages carry metadata only. Capture and navigation remain app requests.
  let previousPresence = "";
  let lastPresence = 0;
  setInterval(() => {
    if (document.hidden) return;
    const state = inspect();
    const signature = JSON.stringify([
      state.status,
      state.bookId,
      state.title,
      state.progress,
      state.fingerprint,
    ]);
    if (signature === previousPresence && Date.now() - lastPresence < 10000)
      return;
    lastPresence = Date.now();
    previousPresence = signature;
    chrome.runtime
      .sendMessage({ protocolVersion: 1, type: "reader-presence" })
      .then((reply) => {
        if (!reply?.ok) previousPresence = "";
      })
      .catch(() => {
        previousPresence = "";
      });
  }, 2000);
})();
