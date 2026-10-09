// Minimal Stream Deck SDK property inspector: loads the action's settings into
// the bound fields and saves them back whenever a field changes.
let socket, context, settings = {};
const bound = [];

function bindSetting(id) {
	bound.push(id);
}

function send(event, payload) {
	socket.send(JSON.stringify({ event, context, payload }));
}

function fill() {
	for (const id of bound) document.getElementById(id).value = settings[id] ?? "";
}

function save() {
	for (const id of bound) settings[id] = document.getElementById(id).value;
	send("setSettings", settings);
}

// deno-lint-ignore no-unused-vars
function connectElgatoStreamDeckSocket(port, uuid, registerEvent, _info, actionInfo) {
	context = uuid;
	settings = JSON.parse(actionInfo).payload.settings ?? {};
	socket = new WebSocket(`ws://127.0.0.1:${port}`);
	socket.onopen = () => socket.send(JSON.stringify({ event: registerEvent, uuid }));
	socket.onmessage = (message) => {
		const data = JSON.parse(message.data);
		if (data.event === "didReceiveSettings") {
			settings = data.payload.settings ?? {};
			fill();
		}
	};
	fill();
	for (const id of bound) document.getElementById(id).addEventListener("change", save);
}
