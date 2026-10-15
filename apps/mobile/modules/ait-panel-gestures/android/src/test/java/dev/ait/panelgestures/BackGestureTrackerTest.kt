package dev.ait.panelgestures

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class BackGestureTrackerTest {
  @Test fun distinguishesBothCancelledEdges() {
    val tracker = BackGestureTracker()
    tracker.startTouch(0f, 400, 24, 24)
    tracker.cancelTouch(100)
    assertEquals("left", tracker.consumeEdge(200))
    assertNull(tracker.consumeEdge(201))
    tracker.startTouch(399f, 400, 24, 24)
    tracker.cancelTouch(300)
    assertEquals("right", tracker.consumeEdge(400))
  }

  @Test fun ignoresInteriorTouchesAndCompletedOrStaleSwipes() {
    val tracker = BackGestureTracker()
    tracker.startTouch(200f, 400, 24, 24)
    tracker.cancelTouch(100)
    assertNull(tracker.consumeEdge(200))
    tracker.startTouch(0f, 400, 24, 24)
    assertNull(tracker.consumeEdge(200))
    tracker.startTouch(0f, 400, 24, 24)
    tracker.cancelTouch(100)
    assertNull(tracker.consumeEdge(3000))
  }

  @Test fun clearsAbortedGesturesBeforeAButtonOrNewTouch() {
    val tracker = BackGestureTracker()
    tracker.startTouch(0f, 400, 24, 24)
    tracker.cancelTouch(100)
    tracker.reset()
    assertNull(tracker.consumeEdge(200))
    tracker.startTouch(0f, 400, 24, 24)
    tracker.cancelTouch(300)
    tracker.startTouch(200f, 400, 24, 24)
    assertNull(tracker.consumeEdge(400))
  }

  @Test fun doesNotClaimEdgesInThreeButtonNavigation() {
    val tracker = BackGestureTracker()
    tracker.startTouch(0f, 400, 0, 0)
    tracker.cancelTouch(100)
    assertNull(tracker.consumeEdge(200))
  }
}
