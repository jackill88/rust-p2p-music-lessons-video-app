await (async function lessonSession() {
  const state = {
    ws: null,
    pc: null,
    stream: null,
    statsTimer: 0,
    partnerPresent: false,
    usingRtc: false,
    facing: "environment",
    muted: false,
    cameraEnabled: true,
    audioSender: null,
    videoSender: null,
    bytesWindow: 0,
    framesWindow: 0,
    lastRtcBytes: 0,
    lastRtcTime: 0,
    audioCtx: null,
    localSource: null,
    localAnalyser: null,
    remoteSource: null,
    remoteAnalyser: null,
    localTime: null,
    localFreq: null,
    remoteTime: null,
    remoteFreq: null,
    feedbackHits: 0,
    feedbackQuiet: 0,
    feedbackActive: false,
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
    const video = $("remote-video");
    const placeholder = $("remote-placeholder");
    if (video) video.style.display = visible ? "block" : "none";
    if (placeholder) placeholder.style.display = visible ? "none" : "grid";
  }

  function clearRemote() {
    const video = $("remote-video");
    if (video) video.srcObject = null;
    setRemoteVisible(false);
  }

  function rtcUnavailable(reason) {
    sendEvent({
      event: "error",
      message: reason || "WebRTC is required for Lesson Studio.",
    });
  }

  function asBool(value, fallback) {
    if (value === true || value === 1 || value === "true" || value === "1") return true;
    if (value === false || value === 0 || value === "false" || value === "0") return false;
    return fallback;
  }

  function mediaStream() {
    return window.__lessonStream || state.stream;
  }

  async function applyMediaFlags() {
    const stream = mediaStream();
    const audioTrack = stream ? stream.getAudioTracks()[0] : null;
    const videoTrack = stream ? stream.getVideoTracks()[0] : null;
    if (audioTrack) audioTrack.enabled = !state.muted;
    if (videoTrack) videoTrack.enabled = state.cameraEnabled;

    if (state.audioSender) {
      try {
        await state.audioSender.replaceTrack(state.muted ? null : audioTrack);
      } catch (_) {
        if (state.audioSender.track) {
          state.audioSender.track.enabled = !state.muted;
        }
      }
    }
    if (state.videoSender) {
      try {
        await state.videoSender.replaceTrack(state.cameraEnabled ? videoTrack : null);
      } catch (_) {
        if (state.videoSender.track) {
          state.videoSender.track.enabled = state.cameraEnabled;
        }
      }
    }
    if (state.muted) {
      publishFeedback(false);
    }
  }

  function ensureAudioContext() {
    const Ctx = window.AudioContext || window.webkitAudioContext;
    if (!Ctx) return null;
    if (!state.audioCtx) {
      state.audioCtx = new Ctx();
    }
    if (state.audioCtx.state === "suspended") {
      state.audioCtx.resume().catch(() => {});
    }
    return state.audioCtx;
  }

  function disconnectTap(kind) {
    const sourceKey = kind === "remote" ? "remoteSource" : "localSource";
    if (state[sourceKey]) {
      try {
        state[sourceKey].disconnect();
      } catch (_) {}
      state[sourceKey] = null;
    }
  }

  function tapStream(kind, stream) {
    const ctx = ensureAudioContext();
    const track = stream && stream.getAudioTracks()[0];
    if (!ctx || !track) return;
    disconnectTap(kind);
    const source = ctx.createMediaStreamSource(new MediaStream([track]));
    const analyser = ctx.createAnalyser();
    analyser.fftSize = 2048;
    analyser.smoothingTimeConstant = 0.65;
    source.connect(analyser);
    if (kind === "remote") {
      state.remoteSource = source;
      state.remoteAnalyser = analyser;
      state.remoteTime = new Uint8Array(analyser.fftSize);
      state.remoteFreq = new Uint8Array(analyser.frequencyBinCount);
    } else {
      state.localSource = source;
      state.localAnalyser = analyser;
      state.localTime = new Uint8Array(analyser.fftSize);
      state.localFreq = new Uint8Array(analyser.frequencyBinCount);
    }
  }

  function stopAudioMonitor() {
    disconnectTap("local");
    disconnectTap("remote");
    state.localAnalyser = null;
    state.remoteAnalyser = null;
    sendEvent({ event: "levels", local: 0, remote: 0 });
    publishFeedback(false);
  }

  function timeRms(analyser, buffer) {
    analyser.getByteTimeDomainData(buffer);
    let sum = 0;
    for (let i = 0; i < buffer.length; i++) {
      const sample = (buffer[i] - 128) / 128;
      sum += sample * sample;
    }
    return Math.sqrt(sum / buffer.length);
  }

  function bandRange(analyser) {
    const rate = (state.audioCtx && state.audioCtx.sampleRate) || 48000;
    const binHz = rate / analyser.fftSize;
    const from = Math.max(1, Math.floor(160 / binHz));
    const to = Math.min(analyser.frequencyBinCount, Math.ceil(5500 / binHz));
    return { from, to };
  }

  function peakInfo(spectrum, from, to) {
    let max = 0;
    let index = from;
    let sum = 0;
    const count = Math.max(1, to - from);
    for (let i = from; i < to; i++) {
      const value = spectrum[i];
      sum += value;
      if (value > max) {
        max = value;
        index = i;
      }
    }
    const mean = sum / count;
    return { max, index, ratio: mean > 4 ? max / mean : 0 };
  }

  function cosine(left, right, from, to) {
    let dot = 0;
    let leftNorm = 0;
    let rightNorm = 0;
    for (let i = from; i < to; i++) {
      const a = left[i];
      const b = right[i];
      dot += a * b;
      leftNorm += a * a;
      rightNorm += b * b;
    }
    if (leftNorm < 8 || rightNorm < 8) return 0;
    return dot / Math.sqrt(leftNorm * rightNorm);
  }

  function publishFeedback(active) {
    if (active === state.feedbackActive && (active || state.feedbackHits === 0)) {
      if (!active) {
        state.feedbackHits = 0;
        state.feedbackQuiet = 0;
      }
      return;
    }
    if (!active) {
      state.feedbackHits = 0;
      state.feedbackQuiet = 0;
    }
    state.feedbackActive = active;
    sendEvent({
      event: "feedback",
      active,
      message: active
        ? "Feedback loop detected. Put on headphones or turn the speaker down."
        : "",
    });
  }

  function sampleAudio() {
    const localAnalyser = state.localAnalyser;
    const localRms =
      localAnalyser && state.localTime ? timeRms(localAnalyser, state.localTime) : 0;
    const remoteAnalyser = state.remoteAnalyser;
    const remoteRms =
      remoteAnalyser && state.remoteTime ? timeRms(remoteAnalyser, state.remoteTime) : 0;
    sendEvent({
      event: "levels",
      local: Math.min(1, localRms * 4.5),
      remote: Math.min(1, remoteRms * 4.5),
    });

    if (state.muted || !localAnalyser || !remoteAnalyser) {
      if (state.feedbackActive) publishFeedback(false);
      return;
    }

    localAnalyser.getByteFrequencyData(state.localFreq);
    remoteAnalyser.getByteFrequencyData(state.remoteFreq);
    const { from, to } = bandRange(localAnalyser);
    const localPeak = peakInfo(state.localFreq, from, to);
    const remotePeak = peakInfo(state.remoteFreq, from, to);
    const similar = cosine(state.localFreq, state.remoteFreq, from, to);
    const sharedPeak = Math.abs(localPeak.index - remotePeak.index) <= 2;
    const looping =
      localRms > 0.045 &&
      remoteRms > 0.045 &&
      similar > 0.84 &&
      sharedPeak &&
      localPeak.ratio > 3 &&
      remotePeak.ratio > 3 &&
      localPeak.max > 132 &&
      remotePeak.max > 132;

    if (looping) {
      state.feedbackHits += 1;
      state.feedbackQuiet = 0;
      if (state.feedbackHits >= 2) {
        publishFeedback(true);
      }
    } else if (state.feedbackActive || state.feedbackHits > 0) {
      state.feedbackQuiet += 1;
      if (state.feedbackQuiet >= 3) {
        publishFeedback(false);
      }
    }
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
    if (state.pc) {
      try {
        state.pc.close();
      } catch (_) {}
      state.pc = null;
    }
    state.audioSender = null;
    state.videoSender = null;
    state.usingRtc = false;
  }

  async function startRtc(initiator) {
    if (!window.RTCPeerConnection || !state.stream) {
      rtcUnavailable("This device cannot start a live WebRTC lesson.");
      return;
    }
    stopRtc();
    const pc = new RTCPeerConnection({
      iceServers: [],
      bundlePolicy: "max-bundle",
    });
    state.pc = pc;
    state.audioSender = null;
    state.videoSender = null;
    state.stream.getTracks().forEach((track) => {
      const sender = pc.addTrack(track, state.stream);
      if (track.kind === "audio") state.audioSender = sender;
      if (track.kind === "video") state.videoSender = sender;
    });
    preferH264(pc);
    await tuneSenders(pc);
    await applyMediaFlags();

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
        video.muted = false;
        video.play().catch(() => {});
      }
      state.usingRtc = true;
      setRemoteVisible(true);
      tapStream("remote", remote);
      sendEvent({ event: "status", message: "Live peer-to-peer (WebRTC)." });
    };
    pc.onconnectionstatechange = () => {
      if (pc.connectionState === "connected") {
        state.usingRtc = true;
        sendEvent({ event: "status", message: "Live peer-to-peer (WebRTC)." });
      }
      if (pc.connectionState === "failed") {
        rtcUnavailable("Live connection failed. Stay on the same Wi-Fi and join again.");
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

  function handleControl(message) {
    sendEvent(message);
    if (message.type === "welcome") {
      state.partnerPresent = Boolean(message.partner);
      if (!state.partnerPresent) {
        clearRemote();
        stopRtc();
        disconnectTap("remote");
        state.remoteAnalyser = null;
        publishFeedback(false);
      }
    } else if (message.type === "partner_joined") {
      state.partnerPresent = true;
      startRtc(true).catch((err) =>
        sendEvent({ event: "error", message: String(err && err.message ? err.message : err) })
      );
    } else if (message.type === "partner_left") {
      state.partnerPresent = false;
      stopRtc();
      disconnectTap("remote");
      state.remoteAnalyser = null;
      publishFeedback(false);
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
    tapStream("local", state.stream);

    const socketUrl = secureSocketUrl(cmd.url);
    await new Promise((resolve, reject) => {
      const ws = new WebSocket(socketUrl);
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
        if (typeof event.data !== "string") return;
        try {
          handleControl(JSON.parse(event.data));
        } catch (err) {
          sendEvent({ event: "error", message: String(err) });
        }
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
      sampleAudio();
      if (!state.usingRtc) {
        state.bytesWindow = 0;
        state.framesWindow = 0;
      }
    }, 1000);
    await applyMediaFlags();
  }

  async function disconnect() {
    stopRtc();
    window.clearInterval(state.statsTimer);
    state.statsTimer = 0;
    state.partnerPresent = false;
    if (state.ws) {
      try {
        if (state.ws.readyState === 1) {
          state.ws.send(JSON.stringify({ type: "leave" }));
        }
        state.ws.close();
      } catch (_) {}
      state.ws = null;
    }
    clearRemote();
    stopAudioMonitor();
  }

  async function applyStream() {
    state.stream = window.__lessonStream || state.stream;
    attachLocalPreview();
    tapStream("local", state.stream);
    const videoTrack = state.stream ? state.stream.getVideoTracks()[0] : null;
    if (videoTrack && state.videoSender && state.cameraEnabled) {
      try {
        await state.videoSender.replaceTrack(videoTrack);
      } catch (_) {}
    }
    await applyMediaFlags();
  }

  window.__lessonSetMuted = (muted) => {
    state.muted = asBool(muted, true);
    return applyMediaFlags();
  };
  window.__lessonSetCamera = (enabled) => {
    state.cameraEnabled = asBool(enabled, true);
    return applyMediaFlags();
  };

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
        state.muted = asBool(cmd.muted, !state.muted);
        await applyMediaFlags();
        sendEvent({
          event: "status",
          message: state.muted ? "Microphone muted." : "Microphone on.",
        });
      } else if (cmd.op === "set_camera") {
        state.cameraEnabled = asBool(cmd.enabled, !state.cameraEnabled);
        await applyMediaFlags();
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
