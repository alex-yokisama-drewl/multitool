// Browser-side helper for the Video Trimmer's "always-available preview".
//
// The trimmer plays the picked source directly when the WebView can decode
// it, and falls back to a transcoded proxy otherwise. `probePlayable`
// answers the "can the WebView decode it?" question at runtime — extension
// guessing is wrong (an .mp4 can hold HEVC the browser refuses, an .mkv
// the browser can't demux at all), so we ask a throwaway <video> element.

// Resolves `true` if the WebView reports decodable video for `url`, else
// `false` (undecodable codec/container, audio-only, or a load error). The
// timeout guards against a source that never fires either event.
export function probePlayable(url: string, timeoutMs = 4000): Promise<boolean> {
  return new Promise((resolve) => {
    const video = document.createElement("video");
    video.preload = "metadata";
    video.muted = true;

    let settled = false;
    const finish = (ok: boolean) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      video.removeEventListener("loadedmetadata", onMeta);
      video.removeEventListener("error", onError);
      // Detach so the element can be GC'd without keeping the stream open.
      video.removeAttribute("src");
      video.load();
      resolve(ok);
    };

    // A decodable video track reports a non-zero intrinsic size. An
    // audio-only or undecodable source reports 0 → treat as not playable
    // so the tool transcodes a proxy.
    const onMeta = () => finish(video.videoWidth > 0);
    const onError = () => finish(false);
    const timer = setTimeout(() => finish(false), timeoutMs);

    video.addEventListener("loadedmetadata", onMeta);
    video.addEventListener("error", onError);
    video.src = url;
  });
}
