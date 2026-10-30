const pages = new URL('./pages/', import.meta.url);
const maxDelayMs = 10_000;

export async function handle(request: Request, port: number): Promise<Response> {
  const url = new URL(request.url);
  const headers = new Headers({ 'x-fixture-port': String(port) });
  const redirect = (location: string) => {
    headers.set('location', location);
    return new Response(null, { status: 302, headers });
  };
  if (url.pathname === '/redirect/path') return redirect('/landing.html');
  if (url.pathname === '/redirect/origin') return redirect(`http://127.0.0.1:${port}/landing.html`);
  if (url.pathname === '/redirect/chain') return redirect('/redirect/path');
  if (url.pathname === '/status/404' || url.pathname === '/status/500') {
    const status = Number(url.pathname.slice(-3));
    return new Response(`Fixture status ${status}`, { status, headers });
  }
  if (url.pathname === '/slow') {
    const raw = url.searchParams.get('ms') ?? '0';
    if (!/^\d+$/.test(raw) || Number(raw) > maxDelayMs) {
      return new Response(`ms must be an integer from 0 to ${maxDelayMs}`, { status: 400, headers });
    }
    await Bun.sleep(Number(raw));
    url.pathname = '/landing.html';
  }
  if (url.pathname === '/set-cookie' || url.pathname === '/read-cookie') {
    const value = url.pathname === '/set-cookie'
      ? crypto.randomUUID()
      : (request.headers.get('cookie')?.split(';').map(part => part.trim()).find(part => part.startsWith('fixture_session='))?.slice('fixture_session='.length) ?? 'none');
    if (url.pathname === '/set-cookie') headers.set('set-cookie', `fixture_session=${value}; Path=/; Max-Age=86400`);
    headers.set('content-type', 'text/html; charset=utf-8');
    // Cookie values are displayed as text, not interpreted as markup.
    const escaped = value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
    return new Response(`<!doctype html><title>Cookie fixture</title><h1 id="whoami"></h1><p id="doc-id"></p><p id="cookie">${escaped}</p><script src="/common.js"></script>`, { headers });
  }
  const name = url.pathname === '/' ? 'landing.html' : url.pathname.slice(1);
  // Only files directly inside pages are served.
  if (!/^[a-z-]+\.(html|js)$/.test(name)) return new Response('Not found', { status: 404, headers });
  const file = Bun.file(new URL(name, pages));
  if (!(await file.exists())) return new Response('Not found', { status: 404, headers });
  headers.set('content-type', name.endsWith('.html') ? 'text/html; charset=utf-8' : 'text/javascript; charset=utf-8');
  return new Response(file, { headers });
}

if (import.meta.main) {
  for (const port of [5173, 3000]) {
    Bun.serve({ hostname: '0.0.0.0', port, fetch: request => handle(request, port) });
    console.log(`Fixture port ${port}: http://localhost:${port} and http://127.0.0.1:${port}`);
  }
}
