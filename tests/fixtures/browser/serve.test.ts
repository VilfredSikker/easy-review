import { describe, expect, test } from 'bun:test';
import { handle } from './serve';

const request = (path: string, init?: RequestInit) => handle(new Request(`http://localhost:3000${path}`, init), 3000);

describe('browser fixture server', () => {
  test('redirects preserve port and chain', async () => {
    for (const [path, location] of [['/redirect/path', '/landing.html'], ['/redirect/origin', 'http://127.0.0.1:3000/landing.html'], ['/redirect/chain', '/redirect/path']]) {
      const response = await request(path);
      expect(response.status).toBe(302);
      expect(response.headers.get('location')).toBe(location);
      expect(response.headers.get('x-fixture-port')).toBe('3000');
    }
  });
  test('serves all pages and shared script', async () => {
    for (const name of ['landing', 'spa', 'form', 'frames', 'frame-a', 'frame-b', 'cross-origin', 'shadow', 'nested-scroll', 'responsive', 'dialog', 'popup', 'hostile', 'canvas', 'password']) {
      const response = await request(`/${name}.html`);
      expect(response.status).toBe(200);
      const html = await response.text();
      expect(html).toContain('id="whoami"');
      expect(html).toContain('id="doc-id"');
      expect(html).toContain('/common.js');
    }
    expect((await request('/common.js')).status).toBe(200);
    expect((await request('/missing.html')).status).toBe(404);
    expect((await request('/%2e%2e/serve.ts')).status).toBe(404);
  });
  test('returns requested status and bounds slow input', async () => {
    expect((await request('/status/404')).status).toBe(404);
    expect((await request('/status/500')).status).toBe(500);
    expect((await request('/slow?ms=0')).status).toBe(200);
    for (const value of ['-1', '1.5', 'NaN', '10001', '']) expect((await request(`/slow?ms=${value}`)).status).toBe(400);
  });
  test('sets and reads a cookie safely', async () => {
    const set = await request('/set-cookie');
    const cookie = set.headers.get('set-cookie')!;
    expect(cookie).toMatch(/^fixture_session=[\w-]+; Path=\/; Max-Age=86400$/);
    const read = await request('/read-cookie', { headers: { cookie: `other=x; ${cookie.split(';')[0]}` } });
    expect(await read.text()).toContain(cookie.split(';')[0].slice('fixture_session='.length));
    expect(await (await request('/read-cookie')).text()).toContain('>none</p>');
    expect(await (await request('/read-cookie', { headers: { cookie: 'fixture_session=<script>' } })).text()).toContain('&lt;script&gt;');
  });
});
