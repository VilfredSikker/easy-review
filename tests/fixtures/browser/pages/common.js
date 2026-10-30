document.querySelector('#whoami').textContent = `Port ${location.port} · ${location.origin}`;
document.querySelector('#doc-id').textContent = `Document ID: ${crypto.randomUUID()}`;
