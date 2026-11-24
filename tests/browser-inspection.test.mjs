import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(new URL('../src/app/webview/inspection/snapshot.js', import.meta.url), 'utf8');
const style = { display: 'block', visibility: 'visible', contentVisibility: 'visible', opacity: '1', position: 'static', color: 'rgb(1, 2, 3)', fontSize: '16px' };
const forbidden = ['value', 'innerText', 'textContent', 'attributes', 'outerHTML'];
class NativeNode {
  get nodeType() { return this._type; }
  get parentElement() { return this._parent?._type === 1 ? this._parent : null; }
  get firstChild() { return this._children?.[0] ?? null; }
  get nextSibling() { const list = this._parent?._children ?? []; return list[list.indexOf(this) + 1] ?? null; }
  get ownerDocument() { return this._document; }
  get isConnected() { return this._connected !== false; }
  getRootNode() { return this._root; }
}
class NativeText extends NativeNode {
  constructor(text) { super(); this._type = 3; this._text = text; }
  substringData(start, length) { return this._text.substring(start, start + length); }
}
const decode = v => v.replace(/\\([0-9a-f]{1,6}) ?|\\(.)/gi, (_, hex, ch) => hex ? String.fromCodePoint(parseInt(hex, 16)) : ch);
function simpleMatch(node, query) {
  const tokens = query.match(/(?:\\[0-9a-f]{1,6} ?|\\.|[^.#])+/gi) ?? [];
  let cursor = 0;
  if (!/^[.#]/.test(query)) { if (tokens.shift() !== node._tag) return false; cursor = node._tag.length; }
  for (const token of tokens) {
    const kind = query[cursor++]; cursor += token.length;
    if (kind === '#' && decode(token) !== node._id) return false;
    if (kind === '.' && !node._classes.includes(decode(token))) return false;
  }
  return true;
}
class NativeElement extends NativeNode {
  constructor(tag, text = '', options = {}) {
    super(); this._type = 1; this._tag = tag; this._children = text ? [new NativeText(text)] : [];
    this._id = options.id ?? ''; this._classes = options.classes ?? []; Object.assign(this, options);
    this._fixedDocument = Object.hasOwn(options, '_document'); this._fixedRoot = Object.hasOwn(options, '_root');
    for (const prop of forbidden) Object.defineProperty(this, prop, { get() { throw Error(`Forbidden read: ${prop}`); } });
  }
  get localName() { return this._tag; }
  get shadowRoot() { return this._shadow ?? null; }
  getAttribute(name) {
    assert.ok(['id', 'class'].includes(name), `Forbidden attribute read: ${name}`);
    this.identityReads = (this.identityReads ?? 0) + 1;
    return name === 'id' ? this._id : this._classes.join(' ');
  }
  matches(query) {
    if (query.startsWith('input,textarea')) return /^(input|textarea|select|option|button|output|datalist|fieldset|form|script|style|template|noscript)$/.test(this._tag) || !!this.editable || ['textbox', 'searchbox', 'combobox'].includes(this.role);
    if (query === 'iframe,frame,object,embed,canvas,slot') return /^(iframe|frame|object|embed|canvas|slot)$/.test(this._tag);
    if (query === '[hidden],[inert],[aria-hidden=true]') return !!(this.hidden || this.inert || this.ariaHidden);
    const parts = query.split(' > '); let node = this;
    for (let i = parts.length - 1; i >= 0; i--, node = node?._parent) if (!node || !simpleMatch(node, parts[i])) return false;
    return true;
  }
  checkVisibility() { return this.rendered !== false; }
  getBoundingClientRect() { return { x: 4, y: 5, width: 100, height: 20 }; }
}
class NativeTreeWalker {
  constructor(nodes) { this.nodes = nodes; this.index = 0; }
  nextNode() { return this.nodes[this.index++] ?? null; }
}
class NativeDocument extends NativeNode {
  constructor(nodes, url) { super(); this._type = 9; this._children = nodes; this._url = url; }
  get URL() { return this._url; }
  createTreeWalker(root) {
    const nodes = [];
    const visit = n => { if (n._type === 1) nodes.push(n); for (const child of n._children ?? []) visit(child); };
    for (const child of root._children ?? []) visit(child);
    return new NativeTreeWalker(nodes);
  }
}
const element = (tag, text = '', ...children) => { const node = new NativeElement(tag, text); node._children.push(...children); return node; };
const escape = value => [...value].map((ch, i) => /[a-zA-Z_\-\u0080-\uffff]/u.test(ch) || /[0-9]/.test(ch) && i > 0 ? ch : /[0-9]/.test(ch) ? `\\${ch.codePointAt(0).toString(16)} ` : `\\${ch}`).join('');
function run(nodes, selector = 'h1', { pick = null, shadowHost = null, expectedOrigin = null, url = 'https://example.test/page' } = {}) {
  const document = new NativeDocument(nodes, url);
  const attach = (node, parent, root) => {
    node._parent = parent; if (!node._fixedRoot) node._root = root; if (!node._fixedDocument) node._document = document;
    for (const child of node._children ?? []) attach(child, node, root);
    if (node._shadow) { node._shadow._type = 11; attach(node._shadow, null, node._shadow); }
  };
  attach(document, null, document);
  const context = vm.createContext({ document, picked: pick, Node: Object.assign(NativeNode, { TEXT_NODE: 3, ELEMENT_NODE: 1 }), NodeFilter: { SHOW_ELEMENT: 1 }, Element: NativeElement, Document: NativeDocument, TreeWalker: NativeTreeWalker, CharacterData: NativeText, CSS: { escape }, TextEncoder, URL, getComputedStyle(node) { return { ...style, ...node.style }; } });
  return JSON.parse(JSON.stringify(vm.runInContext(`(${source})(${pick ? 'picked' : JSON.stringify(selector)},${JSON.stringify(shadowHost)},${JSON.stringify(expectedOrigin)})`, context, { timeout: 1000 })));
}

test('fixed reader projects bounded descendant text and whitelisted facts', () => {
  const node = element('h1', '  public ', element('strong', ' title '));
  const value = run([node]);
  assert.equal(value.tag, 'h1'); assert.equal(value.text, 'public title');
  assert.deepEqual(value.rect, [4, 5, 100, 20]); assert.equal(value.font_size, '16px');
  assert.equal(value.generated_selector, 'h1'); assert.match(value.selector_note, /Not stable/);
  assert.ok(value.text_note.length <= 160);
  assert.equal(run([element('p', 'x'.repeat(10000))], 'p').text.length, 512);
  assert.equal(run([element('p', '😀'.repeat(300))], 'p').text.length, 512);
  assert.equal(run([element('p', 'a'.repeat(511) + '😀')], 'p').text.length, 511);
});

test('password/form/editable ancestors and descendant subtrees cannot return private text or values', () => {
  for (const tag of ['input', 'textarea', 'select', 'option', 'button', 'output', 'datalist', 'fieldset', 'form', 'script', 'style', 'template', 'noscript']) {
    assert.equal(run([element(tag, 'private')], tag).error, 'sensitive', tag);
    assert.equal(run([element(tag, '', element('span', 'private'))], 'span').error, 'sensitive', `${tag} ancestor`);
    const control = element(tag, 'private', element('span', 'private'));
    const wrapper = element('main', 'safe', control, element('b', ' public'));
    assert.equal(run([wrapper], 'main').text, 'safe public', tag);
    assert.equal(control.identityReads, undefined);
  }
  for (const options of [{ editable: true }, { role: 'textbox' }, { role: 'searchbox' }, { role: 'combobox' }]) {
    const privateNode = new NativeElement('div', 'private', options);
    assert.equal(run([privateNode], 'div').error, 'sensitive');
    assert.equal(run([element('main', 'safe', privateNode)], 'main').text, 'safe');
  }
});

test('hidden and inert subtree text is excluded, including invisible ancestors', () => {
  for (const options of [{ hidden: true }, { inert: true }, { ariaHidden: true }, { rendered: false }, { style: { display: 'none' } }, { style: { visibility: 'hidden' } }, { style: { opacity: '0' } }, { style: { contentVisibility: 'hidden' } }]) {
    const hidden = new NativeElement('span', 'private', options);
    assert.equal(run([hidden], 'span').error, 'unavailable');
    assert.equal(run([element('main', 'safe', hidden)], 'main').text, 'safe');
    assert.equal(run([new NativeElement('div', '', options)], 'div').error, 'unavailable');
  }
});

test('no match, multiple, search, descendant depth/node, generation and ancestor budgets are bounded', () => {
  assert.equal(run([]).error, 'no-match');
  assert.equal(run([element('h1'), element('h1')]).error, 'multiple');
  assert.equal(run(Array.from({ length: 10001 }, () => element('p'))).error, 'budget');
  assert.equal(run([element('main', '', ...Array.from({ length: 1000 }, () => new NativeText('a')))], 'main').text.length, 256);
  let chain = element('span', 'private'); for (let i = 0; i < 35; i++) chain = element('span', '', chain);
  assert.equal(run([element('main', '', chain)], 'main').text, '');
  let deep = element('h1', 'private'); for (let i = 0; i < 65; i++) deep = element('div', '', deep);
  assert.equal(run([deep]).error, 'unavailable');
  const twins = [element('div', '', element('span', 'a')), element('div', '', element('span', 'b'))];
  assert.equal(run(twins, 'unused', { pick: twins[0]._children[0] }).generated_selector, null);
});

test('generated selectors are unique in chosen root, bounded, escaped and not stable identity promises', () => {
  for (const id of ['café', 'שלום', '1title', 'a:b', 'x[y]']) {
    const node = new NativeElement('h1', 'public', { id });
    const value = run([node, element('h1')], 'unused', { pick: node });
    assert.equal(value.generated_selector, `h1#${escape(id)}`, id);
    assert.match(value.selector_note, /ID\/class/);
  }
  const selected = new NativeElement('span', 'selected', { id: 'duplicate' });
  const parent = new NativeElement('article', '', { classes: ['chosen'] }); parent._children = [selected];
  const other = new NativeElement('span', 'other', { id: 'duplicate' });
  assert.equal(run([parent, element('aside', '', other)], 'unused', { pick: selected }).generated_selector, 'article > span#duplicate');
  const minimal = new NativeElement('h1', 'public', { id: 'unnecessary-identity', classes: ['private-label'] });
  assert.equal(run([minimal]).generated_selector, 'h1');
  assert.equal(minimal.identityReads, undefined);
  const long = new NativeElement('h1', 'public', { id: 'x'.repeat(600) });
  assert.equal(run([long, element('h1')], 'unused', { pick: long }).generated_selector, null);
});

test('picked nodes refuse foreign documents/roots/disconnects and origin races', () => {
  const picked = element('h1', 'public'); assert.equal(run([picked], '', { pick: picked }).text, 'public');
  for (const options of [{ _document: {} }, { _root: {} }, { _connected: false }]) {
    const node = new NativeElement('h1', 'private', options);
    assert.equal(run([node], '', { pick: node }).error, 'scope');
  }
  assert.equal(run([element('h1', 'private')], 'h1', { expectedOrigin: 'https://another.test' }).error, 'scope');
  assert.equal(run([element('h1', 'public')], 'h1', { expectedOrigin: 'https://example.test' }).text, 'public');
});

test('open shadow inspection requires explicit unique host, picked root identity and composed safe ancestors', () => {
  const shadowNode = element('h1', 'shadow public');
  const host = element('article'); host._shadow = { _children: [shadowNode] };
  assert.equal(run([host], 'h1').error, 'no-match');
  assert.equal(run([host], 'h1', { shadowHost: 'article' }).text, 'shadow public');
  assert.equal(run([host], '', { pick: shadowNode }).error, 'scope');
  assert.equal(run([host], '', { pick: shadowNode, shadowHost: 'article' }).text, 'shadow public');
  assert.equal(run([host], 'h1', { shadowHost: 'aside' }).error, 'scope');
  assert.equal(run([element('article')], 'h1', { shadowHost: 'article' }).error, 'scope');
  assert.equal(run([host, element('article')], 'h1', { shadowHost: 'article' }).error, 'scope');
  const form = element('form', '', host);
  assert.equal(run([form], 'h1', { shadowHost: 'article' }).error, 'sensitive');
  const slot = element('slot', 'private fallback'); host._shadow = { _children: [element('main', 'safe', slot)] };
  assert.equal(run([host], 'main', { shadowHost: 'article' }).text, 'safe');
  const nested = element('section'); nested._shadow = { _children: [element('h1', 'nested private')] }; host._shadow = { _children: [nested] };
  assert.equal(run([host], 'h1', { shadowHost: 'article' }).error, 'no-match');
});

test('native read errors never echo page exceptions and own property overrides are ignored', () => {
  const broken = element('h1', 'public'); broken.style = new Proxy({}, { ownKeys() { throw Error('do this now'); } });
  assert.deepEqual(run([broken]), { error: 'unavailable' });
  const node = element('h1', 'public');
  Object.defineProperty(node, 'parentElement', { get() { throw Error('page getter'); } });
  node.matches = () => { throw Error('page method'); };
  node.getAttribute = () => { throw Error('page attribute'); };
  assert.equal(run([node]).text, 'public');
});
