package dev.ait.panelgestures

/** Older Android versions expose Back without its originating edge. Only retain a
 * system-cancelled edge touch, never an ordinary completed swipe or a stale touch. */
internal class BackGestureTracker {
  private var touchEdge: String? = null
  private var cancelledAt: Long? = null

  fun startTouch(x: Float, width: Int, leftInset: Int, rightInset: Int): Boolean {
    cancelledAt = null
    touchEdge = when {
      x <= leftInset && leftInset > 0 -> "left"
      x >= width - rightInset && rightInset > 0 -> "right"
      else -> null
    }
    return touchEdge != null
  }

  fun cancelTouch(time: Long) {
    cancelledAt = time
  }

  fun reset() {
    touchEdge = null
    cancelledAt = null
  }

  fun consumeEdge(time: Long): String? {
    val cancelled = cancelledAt
    val edge = if (cancelled != null && time - cancelled in 0..2500) touchEdge else null
    reset()
    return edge
  }
}
