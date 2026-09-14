await (async function lessonSession() {
  const state = {
    ws: null,
    stream: null,
    videoTimer: 0,
    audio: null,
    partnerPresent: false,
    facing: "environment",
    muted: false,
    cameraEnabled: true,
    statsTimer: 0,
    bytesWindow: 0,
    framesWindow: 0,
    remoteObjectUrl: "",
  };

  function sendEvent(event) {
    dioxus.send(event);
  }

  function secureSocketUrl(url) {
    if (!url) return url;
    const pageIsSecure =
      typeof location !== "undefined" &&
      location.protocol !== "http:" &&
      location.protocol !== "ws:";
    if (pageIsSecure && url.indexOf("ws://") === 0) {
      return "wss://" + url.slice("ws://".length);
    }
    return url;
  }

  function $(id) {
    return document.getElementById(id);
  }

  function stopTracks(stream) {
    if (!stream) return;
    stream.getTracks().forEach((track) => track.stop());
  }

  function attachLocalPreview() {
    const video = $("local-preview");
    if (video && state.stream) {
      video.srcObject = state.stream;
      video.muted = true;
      video.playsInline = true;
      video.autoplay = true;
      video.play().catch(() => {});
    }
    const pip = $("local-pip");
    if (pip) {
      pip.classList.toggle("mirror", state.facing === "user");
    }
  }

  function setRemoteVisible(visible) {
    const img = $("remote-frame");
    const placeholder = $("remote-placeholder");
    if (img) img.style.display = visible ? "block" : "none";
    if (placeholder) placeholder.style.display = visible ? "none" : "grid";
  }

  function clearRemote() {
    const img = $("remote-frame");
    if (img) {
      img.removeAttribute("src");
    }
    if (state.remoteObjectUrl) {
      URL.revokeObjectURL(state.remoteObjectUrl);
      state.remoteObjectUrl = "";
    }
    setRemoteVisible(false);
  }

  async function ensureAudio() {
    if (state.audio) return state.audio;

    const ctx = new (window.AudioContext || window.webkitAudioContext)({
      sampleRate: 48000,
      latencyHint: "interactive",
    });
    if (ctx.state === "suspended") {
      await ctx.resume();
    }

    const playQueue = [];
    let playOffset = 0;
    const playback = ctx.createScriptProcessor(1024, 0, 2);
    playback.onaudioprocess = (event) => {
      const left = event.outputBuffer.getChannelData(0);
      const right = event.outputBuffer.getChannelData(1);
      for (let i = 0; i < left.length; i += 1) {
        if (!playQueue.length) {
          left[i] = 0;
          right[i] = 0;
          continue;
        }
        const current = playQueue[0];
        left[i] = current.left[playOffset] || 0;
        right[i] = current.right[playOffset] || left[i];
        playOffset += 1;
        if (playOffset >= current.left.length) {
          playQueue.shift();
          playOffset = 0;
        }
      }
    };
    playback.connect(ctx.destination);

    let capture = null;
    let source = null;
    let analyser = null;

    const audio = {
      ctx,
      playback,
      playQueue,
      capture,
      source,
      analyser,
      pendingL: [],
      pendingR: [],
    };
    state.audio = audio;
    return audio;
  }

  function floatToS16(floatSamples) {
    const pcm = new Int16Array(floatSamples.length);
    for (let i = 0; i < floatSamples.length; i += 1) {
      const s = Math.max(-1, Math.min(1, floatSamples[i]));
      pcm[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
    }
    return pcm;
  }

  function s16ToFloat(bytes, channels) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const samples = bytes.byteLength / 2;
    const frames = Math.floor(samples / channels);
    const left = new Float32Array(frames);
    const right = new Float32Array(frames);
    for (let i = 0; i < frames; i += 1) {
      left[i] = view.getInt16((i * channels) * 2, true) / 0x8000;
      right[i] =
        channels > 1
          ? view.getInt16((i * channels + 1) * 2, true) / 0x8000
          : left[i];
    }
    return { left, right };
  }

  function rms(samples) {
    if (!samples.length) return 0;
    let sum = 0;
    for (let i = 0; i < samples.length; i += 1) {
      sum += samples[i] * samples[i];
    }
    return Math.min(1, Math.sqrt(sum / samples.length) * 3);
  }

  async function startCapture() {
    const audio = await ensureAudio();
    if (audio.source) {
      try {
        audio.source.disconnect();
      } catch (_) {}
    }
    if (audio.capture) {
      try {
        audio.capture.disconnect();
      } catch (_) {}
    }

    const source = audio.ctx.createMediaStreamSource(state.stream);
    const analyser = audio.ctx.createAnalyser();
    analyser.fftSize = 512;
    const capture = audio.ctx.createScriptProcessor(1024, 2, 2);
    capture.onaudioprocess = (event) => {
      if (!state.partnerPresent || state.muted || !state.ws || state.ws.readyState !== 1) {
        return;
      }
      const input = event.inputBuffer;
      const left = input.getChannelData(0);
      const right = input.numberOfChannels > 1 ? input.getChannelData(1) : left;
      audio.pendingL.push(new Float32Array(left));
      audio.pendingR.push(new Float32Array(right));

      let count = audio.pendingL.reduce((sum, chunk) => sum + chunk.length, 0);
      const frameSize = Math.round(audio.ctx.sampleRate * 0.02);
      if (count < frameSize) return;

      const mergedL = new Float32Array(count);
      const mergedR = new Float32Array(count);
      let offset = 0;
      for (let i = 0; i < audio.pendingL.length; i += 1) {
        mergedL.set(audio.pendingL[i], offset);
        mergedR.set(audio.pendingR[i], offset);
        offset += audio.pendingL[i].length;
      }
      audio.pendingL = [mergedL.subarray(frameSize)];
      audio.pendingR = [mergedR.subarray(frameSize)];

      const pcm = new Int16Array(frameSize * 2);
      for (let i = 0; i < frameSize; i += 1) {
        const ls = Math.max(-1, Math.min(1, mergedL[i]));
        const rs = Math.max(-1, Math.min(1, mergedR[i]));
        pcm[i * 2] = ls < 0 ? ls * 0x8000 : ls * 0x7fff;
        pcm[i * 2 + 1] = rs < 0 ? rs * 0x8000 : rs * 0x7fff;
      }

      const packet = new Uint8Array(10 + pcm.byteLength);
      const view = new DataView(packet.buffer);
      packet[0] = 2;
      view.setUint32(1, Date.now() >>> 0);
      packet[5] = 2;
      view.setUint32(6, Math.round(audio.ctx.sampleRate));
      packet.set(new Uint8Array(pcm.buffer), 10);
      state.ws.send(packet);
      state.bytesWindow += packet.byteLength;
    };

    source.connect(analyser);
    source.connect(capture);
    const mute = audio.ctx.createGain();
    mute.gain.value = 0;
    capture.connect(mute);
    mute.connect(audio.ctx.destination);

    audio.source = source;
    audio.capture = capture;
    audio.analyser = analyser;
  }

  function startVideoPump() {
    window.clearInterval(state.videoTimer);
    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d", { alpha: false });
    state.videoTimer = window.setInterval(async () => {
      if (!state.partnerPresent || !state.cameraEnabled) return;
      if (!state.ws || state.ws.readyState !== 1) return;
      const video = $("local-preview");
      if (!video || !video.videoWidth) return;
      const maxW = 1280;
      const scale = Math.min(1, maxW / video.videoWidth);
      canvas.width = Math.max(2, Math.round(video.videoWidth * scale) & ~1);
      canvas.height = Math.max(2, Math.round(video.videoHeight * scale) & ~1);
      ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
      const blob = await new Promise((resolve) =>
        canvas.toBlob(resolve, "image/jpeg", 0.72)
      );
      if (!blob) return;
      const jpeg = new Uint8Array(await blob.arrayBuffer());
      const packet = new Uint8Array(9 + jpeg.byteLength);
      const view = new DataView(packet.buffer);
      packet[0] = 1;
      view.setUint32(1, Date.now() >>> 0);
      view.setUint16(5, canvas.width);
      view.setUint16(7, canvas.height);
      packet.set(jpeg, 9);
      state.ws.send(packet);
      state.bytesWindow += packet.byteLength;
      state.framesWindow += 1;
    }, 1000 / 15);
  }

  function handleBinary(buffer) {
    const bytes = new Uint8Array(buffer);
    if (bytes.length < 2) return;
    if (bytes[0] === 1) {
      const jpeg = bytes.subarray(9);
      const blob = new Blob([jpeg], { type: "image/jpeg" });
      const url = URL.createObjectURL(blob);
      const img = $("remote-frame");
      if (img) {
        const previous = state.remoteObjectUrl;
        img.onload = () => {
          if (previous) URL.revokeObjectURL(previous);
        };
        img.src = url;
        state.remoteObjectUrl = url;
        setRemoteVisible(true);
      }
      return;
    }
    if (bytes[0] === 2 && bytes.length >= 10) {
      const channels = bytes[5];
      const pcm = bytes.subarray(10);
      const converted = s16ToFloat(pcm, channels || 1);
      if (state.audio) {
        if (state.audio.playQueue.length > 12) {
          state.audio.playQueue.splice(0, state.audio.playQueue.length - 8);
        }
        state.audio.playQueue.push(converted);
        sendEvent({
          event: "levels",
          local: localLevel(),
          remote: rms(converted.left),
        });
      }
    }
  }

  function localLevel() {
    if (!state.audio || !state.audio.analyser) return 0;
    const data = new Uint8Array(state.audio.analyser.fftSize);
    state.audio.analyser.getByteTimeDomainData(data);
    let sum = 0;
    for (let i = 0; i < data.length; i += 1) {
      const v = (data[i] - 128) / 128;
      sum += v * v;
    }
    return Math.min(1, Math.sqrt(sum / data.length) * 3);
  }

  function handleControl(message) {
    sendEvent(message);
    if (message.type === "welcome") {
      state.partnerPresent = Boolean(message.partner);
      if (!state.partnerPresent) {
        clearRemote();
      }
    } else if (message.type === "partner_joined") {
      state.partnerPresent = true;
    } else if (message.type === "partner_left") {
      state.partnerPresent = false;
      clearRemote();
    }
  }

  async function connect(cmd) {
    await disconnect();
    state.facing = cmd.facing || "environment";
    state.muted = false;
    state.cameraEnabled = true;
    state.partnerPresent = false;
    state.stream = window.__lessonStream || null;
    if (!state.stream) {
      throw new Error("Camera and microphone were not started");
    }
    attachLocalPreview();

    const socketUrl = secureSocketUrl(cmd.url);
    await new Promise((resolve, reject) => {
      const ws = new WebSocket(socketUrl);
      ws.binaryType = "arraybuffer";
      state.ws = ws;
      ws.onopen = () => {
        ws.send(JSON.stringify(cmd.join));
        sendEvent({ event: "status", message: "Connected to the studio server." });
        resolve();
      };
      ws.onerror = () =>
        reject(new Error("Could not reach the studio server at " + socketUrl));
      ws.onclose = () => {
        sendEvent({ event: "status", message: "Disconnected from the studio server." });
      };
      ws.onmessage = (event) => {
        if (typeof event.data === "string") {
          try {
            handleControl(JSON.parse(event.data));
          } catch (err) {
            sendEvent({ event: "error", message: String(err) });
          }
          return;
        }
        handleBinary(event.data);
      };
    });

    try {
      await startCapture();
    } catch (err) {
      sendEvent({
        event: "error",
        message: "Connected, but studio audio failed: " + String(err && err.message ? err.message : err),
      });
    }
    startVideoPump();

    window.clearInterval(state.statsTimer);
    state.statsTimer = window.setInterval(() => {
      sendEvent({
        event: "stats",
        kbps: Math.round((state.bytesWindow * 8) / 1000),
        fps: state.framesWindow,
      });
      state.bytesWindow = 0;
      state.framesWindow = 0;
      sendEvent({ event: "levels", local: localLevel(), remote: 0 });
    }, 1000);
  }

  async function disconnect() {
    window.clearInterval(state.videoTimer);
    window.clearInterval(state.statsTimer);
    state.videoTimer = 0;
    state.statsTimer = 0;
    state.partnerPresent = false;
    if (state.ws) {
      try {
        state.ws.close();
      } catch (_) {}
      state.ws = null;
    }
    if (state.audio) {
      try {
        state.audio.capture && state.audio.capture.disconnect();
        state.audio.source && state.audio.source.disconnect();
      } catch (_) {}
    }
    clearRemote();
  }

  function applyStream() {
    state.stream = window.__lessonStream || state.stream;
    attachLocalPreview();
    if (state.stream) {
      startCapture();
    }
  }

  while (true) {
    const cmd = await dioxus.recv();
    try {
      if (cmd.op === "connect") {
        await connect(cmd);
      } else if (cmd.op === "disconnect") {
        await disconnect();
        stopTracks(window.__lessonStream);
        window.__lessonStream = null;
        state.stream = null;
      } else if (cmd.op === "set_muted") {
        state.muted = Boolean(cmd.muted);
        if (state.stream) {
          state.stream.getAudioTracks().forEach((track) => {
            track.enabled = !state.muted;
          });
        }
      } else if (cmd.op === "set_camera") {
        state.cameraEnabled = Boolean(cmd.enabled);
        if (state.stream) {
          state.stream.getVideoTracks().forEach((track) => {
            track.enabled = state.cameraEnabled;
          });
        }
      } else if (cmd.op === "chat") {
        if (state.ws && state.ws.readyState === 1) {
          state.ws.send(JSON.stringify({ type: "chat", text: cmd.text || "" }));
        }
      } else if (cmd.op === "use_stream") {
        applyStream();
      }
    } catch (err) {
      sendEvent({
        event: "error",
        message: String(err && err.message ? err.message : err),
      });
    }
  }
})();
