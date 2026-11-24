function (selector, shadowHost, expectedOrigin) {
  "use strict";
  shadowHost ??= null;
  expectedOrigin ??= null;
  // Fixed reader in a fresh isolated world, not a renderer bridge or a sandbox.
  // Native getters/methods only. ID/class are the sole attribute reads, used
  // only for bounded selector identity. No values, HTML, listeners or actions.
  try {
    const get = (proto, name, node) => Object.getOwnPropertyDescriptor(proto, name).get.call(node);
    const type = node => get(Node.prototype, "nodeType", node);
    const parent = node => get(Node.prototype, "parentElement", node);
    const first = node => get(Node.prototype, "firstChild", node);
    const next = node => get(Node.prototype, "nextSibling", node);
    const tag = node => get(Element.prototype, "localName", node);
    const matches = (node, query) => Element.prototype.matches.call(node, query);
    const rootOf = node => Node.prototype.getRootNode.call(node);
    if (expectedOrigin !== null && (typeof expectedOrigin !== "string" || new URL(get(Document.prototype, "URL", document)).origin !== expectedOrigin)) return { error: "scope" };
    let root = document, host = null;
    const find = (query, searchRoot) => {
      const walker = Document.prototype.createTreeWalker.call(document, searchRoot, NodeFilter.SHOW_ELEMENT);
      let node, found = null, visited = 0;
      while ((node = TreeWalker.prototype.nextNode.call(walker))) {
        if (++visited > 10000) return { error: "budget" };
        if (matches(node, query)) {
          if (found) return { error: "multiple" };
          found = node;
        }
      }
      return found ? { node: found } : { error: "no-match" };
    };
    const sensitive = "input,textarea,select,option,button,output,datalist,fieldset,form,[contenteditable],[role=textbox],[role=searchbox],[role=combobox],script,style,template,noscript";
    const readable = node => {
      if (matches(node, sensitive)) return "sensitive";
      // Replaced/fallback/slot content isn't reliable rendered descendant text.
      if (matches(node, "iframe,frame,object,embed,canvas,slot")) return "unavailable";
      if (matches(node, "[hidden],[inert],[aria-hidden=true]")) return "unavailable";
      if (!Element.prototype.checkVisibility.call(node, { checkOpacity: true, checkVisibilityCSS: true, contentVisibilityAuto: true })) return "unavailable";
      const style = getComputedStyle(node);
      return style.display === "none" || style.visibility !== "visible" || style.contentVisibility === "hidden" || Number(style.opacity) === 0 ? "unavailable" : null;
    };
    const ancestors = node => {
      let depth = 0;
      while (node) {
        if (++depth > 64) return "unavailable";
        const error = readable(node);
        if (error) return error;
        const p = parent(node);
        node = p || (host && root !== document && rootOf(node) === root ? host : null);
      }
      return null;
    };
    if (shadowHost !== null) {
      if (typeof shadowHost !== "string") return { error: "scope" };
      const result = find(shadowHost, document);
      if (result.error) return { error: "scope" };
      host = result.node;
      const error = ancestors(host);
      if (error) return { error };
      root = get(Element.prototype, "shadowRoot", host);
      if (!root) return { error: "scope" };
    }
    let selected;
    if (typeof selector === "string") {
      const result = find(selector, root);
      if (result.error) return { error: result.error };
      selected = result.node;
    } else {
      selected = selector;
      // A resolved node must still belong to this exact selected frame/root.
      if (!selected || type(selected) !== Node.ELEMENT_NODE || get(Node.prototype, "ownerDocument", selected) !== document || rootOf(selected) !== root || !get(Node.prototype, "isConnected", selected)) return { error: "scope" };
    }
    const error = ancestors(selected);
    if (error) return { error };
    // Descendant text only from visible non-form/non-editable subtrees. Never
    // use textContent/innerText, accessibility names, values or arbitrary attrs.
    let text = "", visited = 0;
    const stack = [];
    let child = first(selected), depth = 1;
    while (child && visited++ < 256 && text.length < 512) {
      let descend = false;
      if (type(child) === Node.TEXT_NODE) text += CharacterData.prototype.substringData.call(child, 0, 512 - text.length);
      else if (type(child) === Node.ELEMENT_NODE && depth < 32 && !readable(child)) descend = true;
      if (descend && first(child)) {
        stack.push([next(child), depth]); child = first(child); depth++;
      } else {
        child = next(child);
        while (!child && stack.length) [child, depth] = stack.pop();
      }
    }
    // Never cut a supplementary Unicode character in half.
    if (/[\uD800-\uDBFF]$/.test(text)) text = text.slice(0, -1);
    let generated = null, candidate = "", ancestor = selected, generationVisits = 0, exhausted = false;
    const unique = query => {
      if (new TextEncoder().encode(query).length > 1024) return false;
      const walker = Document.prototype.createTreeWalker.call(document, root, NodeFilter.SHOW_ELEMENT);
      let node, count = 0, found = null;
      while ((node = TreeWalker.prototype.nextNode.call(walker))) {
        if (++generationVisits > 10000) { exhausted = true; return false; }
        if (matches(node, query)) { count++; found = node; if (count > 1) return false; }
      }
      return count === 1 && found === selected;
    };
    generation: for (let components = 0; ancestor && components < 6; components++, ancestor = parent(ancestor)) {
      const name = tag(ancestor);
      if (!/^[a-z][a-z0-9-]{0,63}$/.test(name)) break;
      let component = name;
      const suffix = candidate ? " > " + candidate : "";
      // Prefer tag-only identity; read ID/classes lazily only if needed.
      if (unique(component + suffix)) { generated = component + suffix; break; }
      if (exhausted) break;
      for (const attr of ["id", "class"]) {
        const raw = Element.prototype.getAttribute.call(ancestor, attr);
        if (typeof raw !== "string" || raw.length > 512) continue;
        const values = attr === "id" ? [raw] : raw.split(/\s+/).slice(0, 3);
        for (const value of values) {
          if (!value || [...value].length > 128 || /[\s\u0000-\u0020\u007f-\u009f]/.test(value)) continue;
          const more = component + (attr === "id" ? "#" : ".") + CSS.escape(value);
          if (new TextEncoder().encode(more + suffix).length > 1024) continue;
          component = more;
          if (unique(component + suffix)) { generated = component + suffix; break generation; }
          if (exhausted) break generation;
        }
      }
      candidate = component + suffix;
    }
    const rect = Element.prototype.getBoundingClientRect.call(selected), style = getComputedStyle(selected);
    return {
      tag: tag(selected).slice(0, 64),
      text: text.replace(/\s+/g, " ").trim(),
      text_note: "Visible descendant text; up to 256 nodes, depth 32 / 512 UTF-16 units. Hidden, form and editable subtrees excluded; may be incomplete.",
      generated_selector: generated,
      selector_note: generated ? "Unique in this frame/root at read time; ID/class identity only. Not stable across page changes." : "No unique selector within six components / 10,000 identity-search visits. Not stable across page changes.",
      rect: [rect.x, rect.y, rect.width, rect.height],
      display: style.display.slice(0, 160), visibility: style.visibility.slice(0, 160),
      position: style.position.slice(0, 160), color: style.color.slice(0, 160), font_size: style.fontSize.slice(0, 160)
    };
  } catch (_) { return { error: "unavailable" }; }
}
