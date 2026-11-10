// Desktop Chromium knows no camera's facing, so `facingMode: {exact: 'environment'}` fails with
// OverconstrainedError and no track reports a facingMode. Orbita's one camera is the film's, and
// it is a phone's back camera; this says so the way Chrome on Android does.
(() => {
  const LABEL = __LABEL__;
  const native = new WeakMap();
  const asked = new WeakMap();
  const toString = Function.prototype.toString;
  // A native method adds no frame to a stack; `body` and the method itself are two.
  const patch = (proto, name, body) => {
    const orig = proto[name];
    const f = { [name](...a) {
      try { return body.call(this, orig, a); } catch (e) { e.stack = e.stack.replace(/(\n    at [^\n]*){2}/, ''); throw e; }
    } }[name];
    native.set(f, toString.call(orig));
    Object.defineProperty(proto, name, { ...Object.getOwnPropertyDescriptor(proto, name), value: f });
  };
  patch(Function.prototype, 'toString', function (orig, a) { return native.has(this) ? native.get(this) : orig.apply(this, a); });

  const wants = f => f == null ? null : typeof f === 'string' || Array.isArray(f) ? null : f.exact;
  const ok = f => { const e = wants(f); return e == null || [].concat(e).includes('environment'); };
  const strip = v => {
    if (!v || typeof v !== 'object') return v;
    const { facingMode, ...rest } = v;
    if (Array.isArray(rest.advanced)) rest.advanced = rest.advanced.map(a => { const { facingMode, ...r } = a; return r; });
    return rest;
  };
  const ours = t => t.kind === 'video' && t.label === LABEL;
  // Chrome's dictionaries come out in key order
  const sorted = o => Object.fromEntries(Object.entries(o).sort(([a], [b]) => a < b ? -1 : 1));

  // not async: a native one throws synchronously on a foreign `this`
  patch(MediaDevices.prototype, 'getUserMedia', function (gum, a) {
    const c = a[0];
    if (!c || !c.video || typeof c.video !== 'object') return gum.apply(this, a);
    if (!ok(c.video.facingMode)) return Promise.reject(new OverconstrainedError('facingMode', ''));
    return gum.call(this, { ...c, video: strip(c.video) }).then(s => {
      for (const t of s.getVideoTracks()) asked.set(t, c.video);
      return s;
    });
  });
  patch(MediaStreamTrack.prototype, 'applyConstraints', function (apply, a) {
    if (!ours(this) || !a[0]) return apply.apply(this, a);
    if (!ok(a[0].facingMode)) return Promise.reject(new OverconstrainedError('facingMode', ''));
    return apply.call(this, strip(a[0])).then(() => { asked.set(this, a[0]); });
  });
  patch(MediaStreamTrack.prototype, 'getSettings', function (get, a) {
    const s = get.apply(this, a);
    return ours(this) ? sorted({ ...s, facingMode: 'environment' }) : s;
  });
  patch(MediaStreamTrack.prototype, 'getCapabilities', function (get, a) {
    const c = get.apply(this, a);
    return ours(this) ? sorted({ ...c, facingMode: ['environment'] }) : c;
  });
  patch(MediaStreamTrack.prototype, 'getConstraints', function (get, a) {
    return asked.has(this) ? structuredClone(asked.get(this)) : get.apply(this, a);
  });
  patch(InputDeviceInfo.prototype, 'getCapabilities', function (get, a) {
    const c = get.apply(this, a);
    return this.label === LABEL ? sorted({ ...c, facingMode: ['environment'] }) : c;
  });
})();
