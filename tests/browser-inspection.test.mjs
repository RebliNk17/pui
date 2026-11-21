import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(new URL('../src/app/webview/inspection/snapshot.js', import.meta.url), 'utf8');
const style = { display: 'block', visibility: 'visible', contentVisibility: 'visible', opacity: '1', position: 'static', color: 'rgb(1, 2, 3)', fontSize: '16px' };
function element(tag, text = '', parent = null) {
  const node = { localName: tag, parentElement: parent, firstChild: { nodeType: 3, text, nextSibling: null }, selected: true };
  for (const prop of ['value', 'innerText', 'textContent', 'attributes', 'outerHTML']) {
    Object.defineProperty(node, prop, { get() { throw Error(`Forbidden read: ${prop}`); } });
  }
  return node;
}
function run(nodes, selector = 'h1') {
  let visited = 0;
  const sandbox = {
    document: { createTreeWalker() { return { nextNode() { return nodes[visited++] ?? null; } }; } },
    NodeFilter: { SHOW_ELEMENT: 1 }, Node: { TEXT_NODE: 3 },
    Element: { prototype: {
      matches(query) {
        if (query === selector) return this.selected;
        return /^(input|textarea|select|option|button|output|datalist|fieldset|form|script|style|template|noscript)$/.test(this.localName) || this.editable;
      },
      getBoundingClientRect() { return { x: 4, y: 5, width: 100, height: 20 }; }
    } },
    CharacterData: { prototype: { substringData(start, length) { return this.text.substring(start, start + length); } } },
    getComputedStyle(node) { return { ...style, ...node.style }; }
  };
  const context = vm.createContext(sandbox);
  const value = vm.runInContext(`(${source})(${JSON.stringify(selector)})`, context, { timeout: 1000 });
  return { value: JSON.parse(JSON.stringify(value)), visited };
}

test('explicit fixed reader projects only direct bounded text and whitelisted facts', () => {
  const node = element('h1', '  public   title  ');
  node.firstChild.nextSibling = { nodeType: 1, text: 'nested secret', nextSibling: null };
  const { value } = run([node]);
  assert.equal(value.tag, 'h1');
  assert.equal(value.text, 'public title');
  assert.deepEqual(value.rect, [4, 5, 100, 20]);
  assert.equal(value.font_size, '16px');
  assert.ok(!JSON.stringify(value).includes('secret'));
  assert.equal(run([element('p', 'x'.repeat(10000))]).value.text.length, 512);
});

test('no match, multiple match, large tree and invisible nodes are honest unavailable states', () => {
  assert.equal(run([]).value.error, 'no-match');
  assert.equal(run([element('h1'), element('h1')]).value.error, 'multiple');
  const nodes = Array.from({ length: 10001 }, () => ({ selected: false }));
  assert.equal(run(nodes).value.error, 'budget');
  const hidden = element('p', 'hidden'); hidden.style = { display: 'none' };
  assert.equal(run([hidden]).value.error, 'unavailable');
  const parent = element('div'); parent.style = { opacity: '0' };
  assert.equal(run([element('span', 'invisible', parent)]).value.error, 'unavailable');
});

test('password/form/editable controls and descendants cannot return text or values', () => {
  for (const tag of ['input', 'textarea', 'select', 'option', 'button', 'output', 'datalist', 'fieldset', 'form', 'script', 'style', 'template', 'noscript']) {
    assert.equal(run([element(tag, 'private')]).value.error, 'sensitive', tag);
    assert.equal(run([element('span', 'private', element(tag))]).value.error, 'sensitive', `${tag} descendant`);
  }
  const editable = element('div', 'private'); editable.editable = true;
  assert.equal(run([editable]).value.error, 'sensitive');
  const wrapper = element('div', 'safe');
  wrapper.firstChild.nextSibling = { nodeType: 1, text: 'password', nextSibling: null };
  assert.equal(run([wrapper]).value.text, 'safe');
});

test('text child traversal is bounded and native read errors do not echo page exceptions', () => {
  const node = element('p'); let child = node.firstChild;
  for (let i = 0; i < 1000; i++) { child.nextSibling = { nodeType: 3, text: 'a', nextSibling: null }; child = child.nextSibling; }
  assert.equal(run([node]).value.text.length, 63);
  const broken = element('p'); Object.defineProperty(broken, 'parentElement', { get() { throw Error('do this now'); } });
  assert.deepEqual(run([broken]).value, { error: 'unavailable' });
});
