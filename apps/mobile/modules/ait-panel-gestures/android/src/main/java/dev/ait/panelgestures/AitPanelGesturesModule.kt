package dev.ait.panelgestures

import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.Window
import android.view.WindowInsets
import android.window.BackEvent
import android.window.OnBackAnimationCallback
import android.window.OnBackInvokedCallback
import android.window.OnBackInvokedDispatcher
import androidx.annotation.RequiresApi
import com.facebook.react.ReactActivity
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

/** Adds edge metadata before RN's ordinary hardwareBackPress dispatch. JS still owns
 * consumption, so sheets, the navigator, and the default Back action keep their order. */
class AitPanelGesturesModule : Module() {
  private val main = Handler(Looper.getMainLooper())
  private val tracker = BackGestureTracker()
  private var enabled = false
  @Volatile private var destroyed = false
  private var installedActivity: ReactActivity? = null
  private var originalCallback: Window.Callback? = null
  private var touchCallback: Window.Callback? = null
  private var unregisterBack: (() -> Unit)? = null

  override fun definition() = ModuleDefinition {
    Name("AitPanelGestures")
    Events("backGesture")

    Function("setEnabled") { value: Boolean ->
      main.post {
        if (destroyed) return@post
        enabled = value
        if (value) attach() else detach()
      }
    }
    OnActivityEntersForeground { main.post { if (enabled) attach() } }
    OnActivityEntersBackground { main.post { detach() } }
    OnActivityDestroys { main.post { detach() } }
    OnDestroy {
      destroyed = true
      main.post { enabled = false; detach() }
    }
  }

  private fun attach() {
    if (destroyed) return
    val activity = appContext.currentActivity as? ReactActivity ?: return
    if (installedActivity === activity) return
    detach()
    val original = activity.window.callback ?: return
    installedActivity = activity
    originalCallback = original
    val callback = object : Window.Callback by original {
      override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        if (touchCallback !== this) return original.dispatchTouchEvent(event)
        when (event.actionMasked) {
          MotionEvent.ACTION_DOWN -> {
            val decor = activity.window.decorView
            val windowInsets = decor.rootWindowInsets
            @Suppress("DEPRECATION")
            val insets = if (Build.VERSION.SDK_INT >= 30) {
              windowInsets?.getInsets(WindowInsets.Type.systemGestures())
            } else {
              windowInsets?.systemGestureInsets
            }
            val location = IntArray(2)
            decor.getLocationOnScreen(location)
            val startsAtEdge = tracker.startTouch(
              event.rawX - location[0], decor.width, insets?.left ?: 0, insets?.right ?: 0
            )
            if (startsAtEdge && Build.VERSION.SDK_INT >= 33) {
              // A newly navigated Fragment can re-register AndroidX's default-priority
              // callback. Observe this edge ahead of it while still staying below the IME.
              unregisterBack?.invoke()
              unregisterBack = registerPlatformBack(activity)
            }
          }
          MotionEvent.ACTION_CANCEL -> tracker.cancelTouch(event.eventTime)
          MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_DOWN -> tracker.reset()
        }
        return original.dispatchTouchEvent(event)
      }

      override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (touchCallback !== this) return original.dispatchKeyEvent(event)
        if (event.keyCode == KeyEvent.KEYCODE_BACK) {
          if (Build.VERSION.SDK_INT >= 33) {
            // API 33+ delivers system edge Back directly to the platform callback. A key
            // here is an actual button and must not inherit an aborted gesture's edge.
            tracker.reset()
          } else if (event.action == KeyEvent.ACTION_UP && !event.isCanceled) {
            publishBack(tracker.consumeEdge(event.eventTime))
          }
        }
        return original.dispatchKeyEvent(event)
      }
    }
    touchCallback = callback
    activity.window.callback = callback
    if (Build.VERSION.SDK_INT >= 33) unregisterBack = registerPlatformBack(activity)
  }

  @RequiresApi(33)
  private fun registerPlatformBack(activity: ReactActivity): () -> Unit {
    var predictiveEdge: String? = null
    val callback: OnBackInvokedCallback = if (Build.VERSION.SDK_INT >= 34) {
      object : OnBackAnimationCallback {
        override fun onBackStarted(event: BackEvent) {
          predictiveEdge = when (event.swipeEdge) {
            BackEvent.EDGE_LEFT -> "left"
            BackEvent.EDGE_RIGHT -> "right"
            else -> null
          }
        }

        override fun onBackCancelled() {
          predictiveEdge = null
          tracker.reset()
        }

        override fun onBackInvoked() {
          dispatchBack(activity, predictiveEdge ?: tracker.consumeEdge(SystemClock.uptimeMillis()))
          predictiveEdge = null
        }
      }
    } else {
      OnBackInvokedCallback {
        dispatchBack(activity, tracker.consumeEdge(SystemClock.uptimeMillis()))
      }
    }
    val dispatcher = activity.onBackInvokedDispatcher
    // DEFAULT stays below the IME, so Back can dismiss the keyboard first.
    dispatcher.registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_DEFAULT, callback)
    return { dispatcher.unregisterOnBackInvokedCallback(callback) }
  }

  @Suppress("DEPRECATION")
  private fun dispatchBack(activity: ReactActivity, edge: String?) {
    tracker.reset()
    publishBack(edge)
    // Call RN's existing entry point, not the platform dispatcher (which would recurse).
    activity.onBackPressed()
  }

  private fun publishBack(edge: String?) {
    if (!destroyed) sendEvent("backGesture", mapOf("edge" to edge))
  }

  private fun detach() {
    unregisterBack?.invoke()
    unregisterBack = null
    installedActivity?.window?.let { window ->
      if (window.callback === touchCallback) window.callback = originalCallback
    }
    installedActivity = null
    originalCallback = null
    touchCallback = null
    tracker.reset()
  }
}
