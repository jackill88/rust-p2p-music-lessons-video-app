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
    levelsEnabled: true,
    meterTimer: 0,
    rtcLocalLevel: 0,
    rtcRemoteLevel: 0,
    gain: 1,
    gainNode: null,
    rawSource: null,
    destination: null,
    processedStream: null,
    preAnalyser: null,
    preTime: null,
    calibrating: false,
    calibrateTimer: 0,
    calibratePeak: 0,
    remoteStream: null,
    previewStream: null,
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
    const stream = window.__lessonStream || state.stream;
    const track = stream && stream.getVideoTracks()[0];
    if (video && track && track.readyState !== "ended") {
      if (!state.previewStream || state.previewStream.getVideoTracks()[0] !== track) {
        state.previewStream = new MediaStream([track]);
      }
      if (video.srcObject !== state.previewStream) {
        video.srcObject = state.previewStream;
      }
      video.muted = true;
      video.playsInline = true;
      video.setAttribute("playsinline", "");
      video.setAttribute("autoplay", "");
      video.autoplay = true;
      video.play().catch(() => {});
    }
    const pip = $("local-pip");
    if (pip) {
      pip.classList.toggle("mirror", state.facing === "user");
    }
  }

  function bindRemoteVideo() {
    const video = $("remote-video");
    const remote = state.remoteStream;
    if (!video || !remote || !remote.getVideoTracks().length) return;
    if (video.srcObject !== remote) {
      video.srcObject = remote;
    }
    video.autoplay = true;
    video.playsInline = true;
    video.muted = false;
    video.setAttribute("playsinline", "");
    video.classList.add("is-live");
    video.play().catch(() => {});
    setRemoteVisible(true);
  }

  function setRemoteVisible(visible) {
    const video = $("remote-video");
    const placeholder = $("remote-placeholder");
    if (video) {
      video.classList.toggle("is-live", visible);
      video.style.display = visible ? "block" : "none";
    }
    if (placeholder) {
      placeholder.classList.toggle("is-hidden", visible);
      placeholder.style.display = visible ? "none" : "grid";
    }
  }

  function clearRemote() {
    const video = $("remote-video");
    state.remoteStream = null;
    if (video) {
      video.srcObject = null;
      video.classList.remove("is-live");
    }
    setRemoteVisible(false);
  }

  function audioOnlyStream(stream) {
    if (!stream) return null;
    const tracks = stream.getAudioTracks().filter((track) => track.readyState !== "ended");
    if (!tracks.length) return null;
    return new MediaStream(tracks);
  }

  function ensureVideoBindings() {
    attachLocalPreview();
    if (state.remoteStream) bindRemoteVideo();
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

  const MIN_GAIN = 0.25;
  const MAX_GAIN = 4;
  const TARGET_PEAK = 0.72;
  const CALIBRATE_MS = 4000;

  function mediaStream() {
    return window.__lessonStream || state.stream;
  }

  function clampGain(value) {
    const number = Number(value);
    if (!Number.isFinite(number)) return 1;
    return Math.min(MAX_GAIN, Math.max(MIN_GAIN, number));
  }

  function outgoingAudioTrack() {
    if (state.processedStream) {
      const processed = state.processedStream.getAudioTracks()[0];
      if (processed) return processed;
    }
    const stream = mediaStream();
    return stream ? stream.getAudioTracks()[0] : null;
  }

  function applyGainValue() {
    if (!state.gainNode) return;
    const value = state.muted ? 0 : state.gain;
    if (state.audioCtx) {
      state.gainNode.gain.setTargetAtTime(value, state.audioCtx.currentTime, 0.02);
    } else {
      state.gainNode.gain.value = value;
    }
  }

  function setGain(value, announce) {
    state.gain = clampGain(value);
    applyGainValue();
    if (announce !== false) {
      sendEvent({ event: "gain", gain: state.gain });
    }
  }

  async function applyMediaFlags() {
    const stream = mediaStream();
    const audioTrack = outgoingAudioTrack();
    const videoTrack = stream ? stream.getVideoTracks()[0] : null;
    if (audioTrack) audioTrack.enabled = !state.muted;
    if (videoTrack) videoTrack.enabled = state.cameraEnabled;
    applyGainValue();

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
      stopCalibrate(true);
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

  ["pointerdown", "touchstart", "click"].forEach((eventName) => {
    window.addEventListener(eventName, ensureAudioContext, true);
  });

  function stopCalibrate(silent) {
    if (state.calibrateTimer) {
      window.clearInterval(state.calibrateTimer);
      state.calibrateTimer = 0;
    }
    state.calibrating = false;
    if (!silent) {
      sendEvent({ event: "calibrate", phase: "idle", remaining: 0, message: "" });
    }
  }

  function teardownLocalGraph() {
    stopCalibrate(true);
    if (state.rawSource) {
      try {
        state.rawSource.disconnect();
      } catch (_) {}
      state.rawSource = null;
    }
    if (state.gainNode) {
      try {
        state.gainNode.disconnect();
      } catch (_) {}
      state.gainNode = null;
    }
    state.destination = null;
    state.processedStream = null;
    state.preAnalyser = null;
    state.preTime = null;
    state.localAnalyser = null;
    state.localTime = null;
    state.localFreq = null;
  }

  function buildLocalAudioGraph() {
    const ctx = ensureAudioContext();
    const stream = mediaStream();
    if (!ctx || !stream || !stream.getAudioTracks().length) return;
    teardownLocalGraph();
    try {
      const audioStream = audioOnlyStream(stream);
      if (!audioStream) return;
      const source = ctx.createMediaStreamSource(audioStream);
      const preAnalyser = ctx.createAnalyser();
      preAnalyser.fftSize = 2048;
      preAnalyser.smoothingTimeConstant = 0;
      const gain = ctx.createGain();
      gain.gain.value = state.muted ? 0 : state.gain;
      const analyser = ctx.createAnalyser();
      analyser.fftSize = 2048;
      analyser.smoothingTimeConstant = 0.4;
      const dest = ctx.createMediaStreamDestination();
      source.connect(preAnalyser);
      source.connect(gain);
      gain.connect(analyser);
      gain.connect(dest);
      state.rawSource = source;
      state.gainNode = gain;
      state.preAnalyser = preAnalyser;
      state.preTime = new Float32Array(preAnalyser.fftSize);
      state.destination = dest;
      state.processedStream = dest.stream;
      state.localAnalyser = analyser;
      state.localTime = new Float32Array(analyser.fftSize);
      state.localFreq = new Uint8Array(analyser.frequencyBinCount);
    } catch (_) {
      tapStream("local", stream);
    }
  }

  function samplePeak(analyser, buffer) {
    if (!analyser || !buffer) return 0;
    analyser.getFloatTimeDomainData(buffer);
    let peak = 0;
    for (let i = 0; i < buffer.length; i++) {
      const magnitude = Math.abs(buffer[i]);
      if (magnitude > peak) peak = magnitude;
    }
    return peak;
  }

  function finishCalibrate() {
    stopCalibrate(true);
    const peak = state.calibratePeak;
    if (peak < 0.08) {
      sendEvent({
        event: "calibrate",
        phase: "fail",
        remaining: 0,
        message: "Too quiet. Play a loud note and try again.",
      });
      return;
    }
    const next = clampGain(TARGET_PEAK / peak);
    setGain(next, true);
    sendEvent({
      event: "calibrate",
      phase: "done",
      remaining: 0,
      gain: next,
      message:
        "Sensitivity set to " +
        next.toFixed(1) +
        "×. Loud notes should sit near the top of the meter.",
    });
  }

  function startCalibrate() {
    if (state.muted) {
      sendEvent({
        event: "calibrate",
        phase: "fail",
        remaining: 0,
        message: "Unmute the microphone before calibrating.",
      });
      return;
    }
    if (!state.preAnalyser) {
      buildLocalAudioGraph();
    }
    if (!state.preAnalyser) {
      sendEvent({
        event: "calibrate",
        phase: "fail",
        remaining: 0,
        message: "Could not listen to the microphone.",
      });
      return;
    }
    stopCalibrate(true);
    state.calibrating = true;
    state.calibratePeak = 0;
    const started = Date.now();
    let lastSecond = 4;
    sendEvent({
      event: "calibrate",
      phase: "play",
      remaining: 4,
      message: "Now play loudly",
    });
    state.calibrateTimer = window.setInterval(() => {
      if (state.muted) {
        stopCalibrate(true);
        sendEvent({
          event: "calibrate",
          phase: "fail",
          remaining: 0,
          message: "Unmute the microphone before calibrating.",
        });
        return;
      }
      const peak = samplePeak(state.preAnalyser, state.preTime);
      if (peak > state.calibratePeak) state.calibratePeak = peak;
      const left = Math.max(0, CALIBRATE_MS - (Date.now() - started));
      const seconds = Math.max(0, Math.ceil(left / 1000));
      if (seconds !== lastSecond) {
        lastSecond = seconds;
        sendEvent({
          event: "calibrate",
          phase: "play",
          remaining: seconds,
          message: "Now play loudly",
        });
      }
      if (left <= 0) {
        finishCalibrate();
      }
    }, 80);
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
    const audioStream = audioOnlyStream(stream);
    if (!ctx || !audioStream) return;
    disconnectTap(kind);
    try {
      const source = ctx.createMediaStreamSource(audioStream);
      const analyser = ctx.createAnalyser();
      analyser.fftSize = 2048;
      analyser.smoothingTimeConstant = 0.4;
      source.connect(analyser);
      const time = new Float32Array(analyser.fftSize);
      const freq = new Uint8Array(analyser.frequencyBinCount);
      if (kind === "remote") {
        state.remoteSource = source;
        state.remoteAnalyser = analyser;
        state.remoteTime = time;
        state.remoteFreq = freq;
      } else {
        state.localSource = source;
        state.localAnalyser = analyser;
        state.localTime = time;
        state.localFreq = freq;
      }
    } catch (_) {}
  }

  function stopAudioMonitor() {
    teardownLocalGraph();
    disconnectTap("remote");
    state.remoteAnalyser = null;
    paintMeter("local-level-fill", 0);
    paintMeter("remote-level-fill", 0);
    publishFeedback(false);
  }

  function timeRms(analyser, buffer) {
    if (!analyser || !buffer) return 0;
    analyser.getFloatTimeDomainData(buffer);
    let sum = 0;
    for (let i = 0; i < buffer.length; i++) {
      const sample = buffer[i];
      sum += sample * sample;
    }
    return Math.sqrt(sum / buffer.length);
  }

  function displayLevel(rms, rtcLevel) {
    const fromRms = Math.min(1, Math.pow(Math.max(0, rms) * 8, 0.65));
    const fromRtc = Math.min(1, Math.max(0, rtcLevel) * 2.4);
    return Math.max(fromRms, fromRtc);
  }

  function paintMeter(id, level) {
    const fill = $(id);
    if (fill) fill.style.width = `${Math.round(Math.max(0, Math.min(1, level)) * 100)}%`;
  }

  function sampleMeters() {
    ensureAudioContext();
    if (!state.levelsEnabled) return;
    const localRms = timeRms(state.localAnalyser, state.localTime);
    const remoteRms = timeRms(state.remoteAnalyser, state.remoteTime);
    paintMeter("local-level-fill", state.muted ? 0 : displayLevel(localRms, state.rtcLocalLevel));
    paintMeter("remote-level-fill", displayLevel(remoteRms, state.rtcRemoteLevel));
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
    buildLocalAudioGraph();
    const videoTrack = state.stream.getVideoTracks()[0];
    const audioTrack = outgoingAudioTrack();
    const sendStream = new MediaStream();
    if (videoTrack) sendStream.addTrack(videoTrack);
    if (audioTrack) sendStream.addTrack(audioTrack);
    if (videoTrack) {
      state.videoSender = pc.addTrack(videoTrack, sendStream);
    }
    if (audioTrack) {
      state.audioSender = pc.addTrack(audioTrack, sendStream);
    }
    preferH264(pc);
    await tuneSenders(pc);
    await applyMediaFlags();

    pc.onicecandidate = (event) => {
      if (event.candidate) {
        sendSignal({ kind: "ice", candidate: event.candidate.toJSON() });
      }
    };
    pc.ontrack = (event) => {
      if (event.streams && event.streams[0]) {
        state.remoteStream = event.streams[0];
      } else {
        if (!state.remoteStream) state.remoteStream = new MediaStream();
        if (event.track && !state.remoteStream.getTracks().includes(event.track)) {
          state.remoteStream.addTrack(event.track);
        }
      }
      state.usingRtc = true;
      bindRemoteVideo();
      const remoteAudio = audioOnlyStream(state.remoteStream);
      if (remoteAudio) tapStream("remote", remoteAudio);
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
    state.levelsEnabled = true;
    state.gain = 1;
    state.partnerPresent = false;
    state.stream = window.__lessonStream || null;
    if (!state.stream) {
      throw new Error("Camera and microphone were not started");
    }
    attachLocalPreview();
    buildLocalAudioGraph();
    sendEvent({ event: "gain", gain: state.gain });

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
            if (typeof report.audioLevel === "number") {
              if (report.type === "media-source" || (report.type === "outbound-rtp" && report.kind === "audio")) {
                state.rtcLocalLevel = report.audioLevel;
              }
              if (
                (report.type === "inbound-rtp" && report.kind === "audio") ||
                report.remoteSource === true
              ) {
                state.rtcRemoteLevel = report.audioLevel;
              }
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
    window.clearInterval(state.meterTimer);
    state.meterTimer = window.setInterval(() => {
      ensureVideoBindings();
      sampleMeters();
    }, 120);
    ensureVideoBindings();
    await applyMediaFlags();
  }

  async function disconnect() {
    stopRtc();
    window.clearInterval(state.statsTimer);
    state.statsTimer = 0;
    window.clearInterval(state.meterTimer);
    state.meterTimer = 0;
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
    state.previewStream = null;
    attachLocalPreview();
    buildLocalAudioGraph();
    const videoTrack = state.stream ? state.stream.getVideoTracks()[0] : null;
    if (videoTrack && state.videoSender && state.cameraEnabled) {
      try {
        await state.videoSender.replaceTrack(videoTrack);
      } catch (_) {}
    }
    const audioTrack = outgoingAudioTrack();
    if (audioTrack && state.audioSender && !state.muted) {
      try {
        await state.audioSender.replaceTrack(audioTrack);
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
  window.__lessonSetLevels = (enabled) => {
    state.levelsEnabled = asBool(enabled, true);
    const meters = $("level-meters");
    if (meters) meters.classList.toggle("is-hidden", !state.levelsEnabled);
    if (!state.levelsEnabled) {
      paintMeter("local-level-fill", 0);
      paintMeter("remote-level-fill", 0);
    } else {
      sampleMeters();
    }
  };

  window.__lessonSetGain = (value) => {
    setGain(value, false);
  };
  window.__lessonCalibrate = () => {
    startCalibrate();
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
      } else if (cmd.op === "set_levels") {
        window.__lessonSetLevels(cmd.enabled);
      } else if (cmd.op === "set_gain") {
        setGain(cmd.value, false);
      } else if (cmd.op === "calibrate") {
        startCalibrate();
      }
    } catch (err) {
      sendEvent({
        event: "error",
        message: String(err && err.message ? err.message : err),
      });
    }
  }
})();
