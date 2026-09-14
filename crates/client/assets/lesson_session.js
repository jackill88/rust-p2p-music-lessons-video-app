await (async function lessonSession() {
  const state = {
    ws: null,
    pc: null,
    stream: null,
    audio: null,
    videoTimer: 0,
    statsTimer: 0,
    iceTimer: 0,
    partnerPresent: false,
    usingRtc: false,
    jpegBusy: false,
    facing: "environment",
    muted: false,
    cameraEnabled: true,
    bytesWindow: 0,
    framesWindow: 0,
    lastRtcBytes: 0,
    lastRtcTime: 0,
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

  function sendSignal(payload) {
    if (!state.ws || state.ws.readyState !== 1) return;
    state.ws.send(JSON.stringify(Object.assign({ type: "signal" }, payload)));
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
    const video = $("remote-video");
    const placeholder = $("remote-placeholder");
    if (video) {
      video.style.display = visible && state.usingRtc ? "block" : "none";
    }
    if (img) {
      img.style.display = visible && !state.usingRtc ? "block" : "none";
    }
    if (placeholder) {
      placeholder.style.display = visible ? "none" : "grid";
    }
  }

  function clearRemote() {
    const img = $("remote-frame");
    const video = $("remote-video");
    if (img) img.removeAttribute("src");
    if (video) {
      video.srcObject = null;
    }
    if (state.remoteObjectUrl) {
      URL.revokeObjectURL(state.remoteObjectUrl);
      state.remoteObjectUrl = "";
    }
    setRemoteVisible(false);
  }

  function preferH264(pc) {
    if (!pc || !window.RTCRtpSender || !RTCRtpSender.getCapabilities) return;
    const caps = RTCRtpSender.getCapabilities("video");
    if (!caps || !caps.codecs) return;
    const preferred = caps.codecs.filter(
      (codec) => /H264/i.test(codec.mimeType) || /VP8/i.test(codec.mimeType)
    );
    if (!preferred.length) return;
    pc.getTransceivers().forEach((transceiver) => {
      if (transceiver.sender && transceiver.sender.track && transceiver.sender.track.kind === "video") {
        try {
          transceiver.setCodecPreferences(preferred);
        } catch (_) {}
      }
    });
  }

  async function tuneSenders(pc) {
    for (const sender of pc.getSenders()) {
      if (!sender.track) continue;
      const params = sender.getParameters();
      params.encodings = params.encodings && params.encodings.length ? params.encodings : [{}];
      if (sender.track.kind === "video") {
        params.encodings[0].maxBitrate = 2_500_000;
        params.encodings[0].maxFramerate = 30;
      } else if (sender.track.kind === "audio") {
        params.encodings[0].maxBitrate = 256_000;
      }
      try {
        await sender.setParameters(params);
      } catch (_) {}
    }
  }

  function stopRtc() {
    window.clearTimeout(state.iceTimer);
    state.iceTimer = 0;
    if (state.pc) {
      try {
        state.pc.close();
      } catch (_) {}
      state.pc = null;
    }
    state.usingRtc = false;
  }

  function startJpegFallback() {
    if (state.videoTimer) return;
    sendEvent({
      event: "status",
      message: "Live over the studio server (JPEG fallback).",
    });
    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d", { alpha: false });
    const pump = () => {
      state.videoTimer = window.requestAnimationFrame(pump);
      if (state.usingRtc || state.jpegBusy) return;
      if (!state.partnerPresent || !state.cameraEnabled) return;
      if (!state.ws || state.ws.readyState !== 1) return;
      const video = $("local-preview");
      if (!video || !video.videoWidth) return;
      state.jpegBusy = true;
      const maxW = 640;
      const scale = Math.min(1, maxW / video.videoWidth);
      canvas.width = Math.max(2, Math.round(video.videoWidth * scale) & ~1);
      canvas.height = Math.max(2, Math.round(video.videoHeight * scale) & ~1);
      ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
      canvas.toBlob(
        async (blob) => {
          try {
            if (!blob || state.usingRtc || !state.ws || state.ws.readyState !== 1) return;
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
          } finally {
            state.jpegBusy = false;
          }
        },
        "image/jpeg",
        0.55
      );
    };
    state.videoTimer = window.requestAnimationFrame(pump);
  }

  function stopJpegFallback() {
    if (state.videoTimer) {
      window.cancelAnimationFrame(state.videoTimer);
      state.videoTimer = 0;
    }
    state.jpegBusy = false;
  }

  async function startRtc(initiator) {
    if (!window.RTCPeerConnection || !state.stream) {
      startJpegFallback();
      return;
    }
    stopRtc();
    stopJpegFallback();
    const pc = new RTCPeerConnection({
      iceServers: [],
      bundlePolicy: "max-bundle",
    });
    state.pc = pc;
    state.stream.getTracks().forEach((track) => pc.addTrack(track, state.stream));
    preferH264(pc);
    await tuneSenders(pc);

    pc.onicecandidate = (event) => {
      if (event.candidate) {
        sendSignal({ kind: "ice", candidate: event.candidate.toJSON() });
      }
    };
    pc.ontrack = (event) => {
      const remote = event.streams[0] || new MediaStream([event.track]);
      const video = $("remote-video");
      if (video) {
        video.srcObject = remote;
        video.autoplay = true;
        video.playsInline = true;
        video.play().catch(() => {});
      }
      state.usingRtc = true;
      stopJpegFallback();
      setRemoteVisible(true);
      sendEvent({ event: "status", message: "Live peer-to-peer (WebRTC)." });
    };
    pc.onconnectionstatechange = () => {
      if (pc.connectionState === "connected") {
        state.usingRtc = true;
        stopJpegFallback();
        sendEvent({ event: "status", message: "Live peer-to-peer (WebRTC)." });
      }
      if (pc.connectionState === "failed" || pc.connectionState === "disconnected") {
        startJpegFallback();
      }
    };

    if (initiator) {
      const offer = await pc.createOffer({
        offerToReceiveAudio: true,
        offerToReceiveVideo: true,
      });
      await pc.setLocalDescription(offer);
      sendSignal({ kind: "offer", sdp: offer.sdp });
    }

    window.clearTimeout(state.iceTimer);
    state.iceTimer = window.setTimeout(() => {
      if (!state.usingRtc) {
        startJpegFallback();
      }
    }, 4000);
  }

  async function handleSignal(message) {
    if (!state.pc && message.kind !== "offer") {
      return;
    }
    if (message.kind === "offer") {
      if (!state.pc) {
        await startRtc(false);
      }
      await state.pc.setRemoteDescription({ type: "offer", sdp: message.sdp });
      const answer = await state.pc.createAnswer();
      await state.pc.setLocalDescription(answer);
      sendSignal({ kind: "answer", sdp: answer.sdp });
      return;
    }
    if (message.kind === "answer" && state.pc) {
      await state.pc.setRemoteDescription({ type: "answer", sdp: message.sdp });
      return;
    }
    if (message.kind === "ice" && state.pc && message.candidate) {
      try {
        await state.pc.addIceCandidate(message.candidate);
      } catch (_) {}
    }
  }

  function handleBinary(buffer) {
    if (state.usingRtc) return;
    const bytes = new Uint8Array(buffer);
    if (bytes.length < 2 || bytes[0] !== 1) return;
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
  }

  function handleControl(message) {
    sendEvent(message);
    if (message.type === "welcome") {
      state.partnerPresent = Boolean(message.partner);
      if (!state.partnerPresent) {
        clearRemote();
        stopRtc();
      }
    } else if (message.type === "partner_joined") {
      state.partnerPresent = true;
      startRtc(true).catch((err) =>
        sendEvent({ event: "error", message: String(err && err.message ? err.message : err) })
      );
    } else if (message.type === "partner_left") {
      state.partnerPresent = false;
      stopRtc();
      stopJpegFallback();
      clearRemote();
    } else if (message.type === "signal") {
      handleSignal(message).catch((err) =>
        sendEvent({ event: "error", message: String(err && err.message ? err.message : err) })
      );
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

    window.clearInterval(state.statsTimer);
    state.statsTimer = window.setInterval(async () => {
      if (state.pc && state.usingRtc) {
        try {
          const stats = await state.pc.getStats();
          stats.forEach((report) => {
            if (report.type === "inbound-rtp" && report.kind === "video") {
              state.framesWindow = Math.round(report.framesPerSecond || 0);
              const received = report.bytesReceived || 0;
              const now = report.timestamp || Date.now();
              if (state.lastRtcTime && now > state.lastRtcTime) {
                const seconds = (now - state.lastRtcTime) / 1000;
                state.bytesWindow = Math.max(0, (received - state.lastRtcBytes) / seconds);
              }
              state.lastRtcBytes = received;
              state.lastRtcTime = now;
            }
          });
        } catch (_) {}
      }
      sendEvent({
        event: "stats",
        kbps: Math.round((state.bytesWindow * 8) / 1000),
        fps: state.framesWindow,
      });
      if (!state.usingRtc) {
        state.bytesWindow = 0;
        state.framesWindow = 0;
      }
    }, 1000);
  }

  async function disconnect() {
    stopJpegFallback();
    stopRtc();
    window.clearInterval(state.statsTimer);
    state.statsTimer = 0;
    state.partnerPresent = false;
    if (state.ws) {
      try {
        state.ws.close();
      } catch (_) {}
      state.ws = null;
    }
    clearRemote();
  }

  async function applyStream() {
    state.stream = window.__lessonStream || state.stream;
    attachLocalPreview();
    if (state.pc && state.stream) {
      const videoTrack = state.stream.getVideoTracks()[0];
      const sender = state.pc.getSenders().find((item) => item.track && item.track.kind === "video");
      if (sender && videoTrack) {
        try {
          await sender.replaceTrack(videoTrack);
        } catch (_) {}
      }
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
        await applyStream();
      }
    } catch (err) {
      sendEvent({
        event: "error",
        message: String(err && err.message ? err.message : err),
      });
    }
  }
})();
