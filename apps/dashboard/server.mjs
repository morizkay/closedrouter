import { createReadStream, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(fileURLToPath(new URL('.', import.meta.url)), 'dist');
const host = process.env.HOST ?? '0.0.0.0';
const port = Number(process.env.PORT ?? 3000);
const mimeTypes = {
	'.css': 'text/css; charset=utf-8',
	'.html': 'text/html; charset=utf-8',
	'.js': 'text/javascript; charset=utf-8',
	'.json': 'application/json; charset=utf-8',
	'.svg': 'image/svg+xml',
	'.woff': 'font/woff',
	'.woff2': 'font/woff2'
};

function safePath(urlPath) {
	try {
		const pathname = decodeURIComponent(urlPath.split('?')[0]);
		const candidate = resolve(root, `.${pathname === '/' ? '/index.html' : pathname}`);
		return candidate === root || candidate.startsWith(`${root}${sep}`) ? candidate : null;
	} catch {
		return null;
	}
}

const server = createServer((request, response) => {
	if (request.method === 'GET' && request.url?.split('?')[0] === '/env.js') {
		response.statusCode = 200;
		response.setHeader('Content-Type', 'text/javascript; charset=utf-8');
		response.setHeader('Cache-Control', 'no-store');
		response.end(
			`window.__CLOSEDROUTER_CONFIG__ = ${JSON.stringify({
				gatewayUrl: process.env.PUBLIC_GATEWAY_URL ?? ''
			})};\n`
		);
		return;
	}

	const requested = safePath(request.url ?? '/');
	if (!requested && request.url) {
		response.statusCode = 400;
		response.setHeader('Content-Type', 'text/plain; charset=utf-8');
		response.end('Bad request');
		return;
	}
	const exists = requested && existsSync(requested) && statSync(requested).isFile();
	const hasFileExtension = extname(request.url?.split('?')[0] ?? '') !== '';
	if (!exists && hasFileExtension) {
		response.statusCode = 404;
		response.setHeader('Content-Type', 'text/plain; charset=utf-8');
		response.end('Not found');
		return;
	}
	const filePath = exists ? requested : join(root, 'index.html');
	const extension = extname(filePath);
	response.setHeader('Content-Type', mimeTypes[extension] ?? 'application/octet-stream');
	response.setHeader(
		'Cache-Control',
		extension === '.html' ? 'no-cache' : 'public, max-age=31536000, immutable'
	);
	createReadStream(filePath)
		.on('error', () => {
			response.statusCode = 500;
			response.end('Internal server error');
		})
		.pipe(response);
});

server.listen(port, host, () => {
	console.log(`ClosedRouter dashboard listening on http://${host}:${port}`);
});
