import { startGui } from './gui.mjs';

function formatErrorForLog(err) {
	if (err == null)
		return 'Unknown error';
	const anyErr = err;
	const name = String(anyErr?.name || 'Error');
	const message = String(anyErr?.message || err);
	const stack = (typeof anyErr?.stack === 'string' && anyErr.stack.length > 0)
		? anyErr.stack
		: null;
	return stack ? `${name}: ${message}\n${stack}` : `${name}: ${message}`;
}

function logErrorWithStack(label, err) {
	const text = `[${label}] ${formatErrorForLog(err)}`;
	console.error(text);
	try {
		const pre = globalThis.document?.createElement?.('pre');
		if (!pre)
			return;
		pre.textContent = text;
		globalThis.document?.body?.appendChild?.(pre);
	}
	catch {
		// Best-effort UI fallback only.
	}
}

if (typeof globalThis.addEventListener === 'function') {
	globalThis.addEventListener('error', (ev) => {
		logErrorWithStack('window.error', ev?.error ?? ev?.message ?? ev);
	});
	globalThis.addEventListener('unhandledrejection', (ev) => {
		logErrorWithStack('unhandledrejection', ev?.reason ?? ev);
	});
}

startGui().catch((err) => {
	logErrorWithStack('main.catch', err);
});
