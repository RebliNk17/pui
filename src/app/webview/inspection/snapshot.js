function (selector) {
  "use strict";
  // Runs only in a CEF isolated world. Never accepts page functions or properties,
  // reads values/attributes, creates DOM, installs listeners, or performs actions.
  try {
    let selected = null;
    if (typeof selector === "string") {
      const walker = document.createTreeWalker(document, NodeFilter.SHOW_ELEMENT);
      let node, visited = 0;
      while ((node = walker.nextNode())) {
        if (++visited > 10000) return { error: "budget" };
        if (Element.prototype.matches.call(node, selector)) {
          if (selected) return { error: "multiple" };
          selected = node;
        }
      }
      if (!selected) return { error: "no-match" };
    } else {
      // Only a CDP-resolved node in this same isolated top-document world.
      selected = selector;
      if (!selected || selected.nodeType !== Node.ELEMENT_NODE || selected.ownerDocument !== document || selected.getRootNode() !== document || !selected.isConnected)
        return { error: "unavailable" };
    }
    const sensitive = "input,textarea,select,option,button,output,datalist,fieldset,form,[contenteditable],[role=textbox],script,style,template,noscript";
    let ancestor = selected, depth = 0;
    while (ancestor) {
      if (++depth > 64) return { error: "unavailable" };
      if (Element.prototype.matches.call(ancestor, sensitive)) return { error: "sensitive" };
      const style = getComputedStyle(ancestor);
      if (style.display === "none" || style.visibility !== "visible" || style.contentVisibility === "hidden" || Number(style.opacity) === 0) return { error: "unavailable" };
      ancestor = ancestor.parentElement;
    }
    const rect = Element.prototype.getBoundingClientRect.call(selected);
    const style = getComputedStyle(selected);
    let text = "", child = selected.firstChild, children = 0;
    while (child && children++ < 64 && text.length < 512) {
      if (child.nodeType === Node.TEXT_NODE) text += CharacterData.prototype.substringData.call(child, 0, 512 - text.length);
      child = child.nextSibling;
    }
    return {
      tag: selected.localName.slice(0, 64),
      text: text.replace(/\s+/g, " ").trim(),
      text_note: "Direct text only, at most 64 children / 512 UTF-16 units. Descendant, hidden, form and editable text excluded; may be incomplete.",
      rect: [rect.x, rect.y, rect.width, rect.height],
      display: style.display.slice(0, 160), visibility: style.visibility.slice(0, 160),
      position: style.position.slice(0, 160), color: style.color.slice(0, 160), font_size: style.fontSize.slice(0, 160)
    };
  } catch (_) { return { error: "unavailable" }; }
}
