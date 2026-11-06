/** Installs a browser-side sampler; readiness and the read run in one JS turn. */
export function installReadyCanvasSampler() {
  window.__captureReadyChemicalCanvas = (svg, expected, read) => new Promise((resolve, reject) => {
    let finished = false
    const observer = new MutationObserver(check)
    const fonts = document.fonts
    const timer = setTimeout(() => finish(new Error(`Canvas measurement did not become ready: ${JSON.stringify({expected,actual:state()})}`)), 30_000)
    function state() {
      const output = document.querySelector('[aria-label="Weight snapshot"]')
      const current = output?.textContent ? JSON.parse(output.textContent) : null
      return { documentId:current?.document.id,revision:current?.document.revision,settings:current?.document.settings,
        ready:svg.dataset.captionBoundsReady,measuredViewBox:svg.dataset.captionMeasuredViewBox,
        paintedViewBox:svg.getAttribute('viewBox'),fonts:fonts.status,busy:svg.classList.contains('cursor-progress'),connected:svg.isConnected }
    }
    function finish(error, result) {
      if (finished) return
      finished = true
      clearTimeout(timer)
      observer.disconnect()
      fonts.removeEventListener('loadingdone', check)
      fonts.removeEventListener('loadingerror', check)
      if (error) reject(error)
      else resolve(result)
    }
    function check() {
      if (finished) return
      try {
        const actual = state()
        if (!actual.connected || actual.documentId !== expected.documentId || actual.revision > expected.revision) {
          finish(new Error(`Canvas sample belongs to another document/revision: ${JSON.stringify({expected,actual})}`))
          return
        }
        if (actual.revision !== expected.revision || actual.ready !== 'true' || actual.busy || actual.fonts !== 'loaded' ||
            actual.measuredViewBox !== actual.paintedViewBox ||
            !Object.entries(expected.settings).every(([field,value]) => actual.settings?.[field] === value)) return
        // Never retry a read or an assertion. A later autosave/render cannot run
        // between this provenance check and the synchronous geometry sample.
        finish(null, read())
      } catch (error) {
        finish(error)
      }
    }
    observer.observe(document.body, {subtree:true,childList:true,characterData:true,attributes:true})
    fonts.addEventListener('loadingdone', check)
    fonts.addEventListener('loadingerror', check)
    check()
  })
}
