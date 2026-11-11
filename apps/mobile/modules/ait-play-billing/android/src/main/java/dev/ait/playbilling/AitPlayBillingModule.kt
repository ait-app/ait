package dev.ait.playbilling

import android.os.Handler
import android.os.Looper
import com.android.billingclient.api.*
import expo.modules.kotlin.Promise
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

/** Billing produces purchase proofs. Only the account server grants/acknowledges paid time. */
class AitPlayBillingModule : Module(), PurchasesUpdatedListener {
  private val main = Handler(Looper.getMainLooper())
  private var client: BillingClient? = null
  private var connecting = false
  private var observing = false
  private val waiting = mutableListOf<Pair<Promise, (BillingClient) -> Unit>>()
  private var purchase: Promise? = null
  private var purchaseProduct: String? = null
  private var purchaseTimeout: Runnable? = null

  override fun definition() = ModuleDefinition {
    Name("AitPlayBilling")
    Events("transaction")
    OnStartObserving { observing = true }
    OnStopObserving { observing = false }
    OnDestroy {
      main.post {
        main.removeCallbacksAndMessages(null)
        rejectPurchase("E_BILLING_CLOSED", "Purchase was interrupted. Restore purchases to retry.")
        waiting.forEach { it.first.reject("E_BILLING_CLOSED", "Billing closed.", null) }
        waiting.clear()
        client?.endConnection()
        client = null
        connecting = false
      }
    }
    AsyncFunction("product") { productId: String, basePlanId: String, promise: Promise ->
      ready(promise) { billing ->
        details(billing, productId, basePlanId, promise) { product, offer ->
          val phase = offer.pricingPhases.pricingPhaseList.singleOrNull()
          if (phase == null) promise.resolve(null)
          else promise.resolve(mapOf("id" to product.productId, "displayPrice" to phase.formattedPrice))
        }
      }
    }
    AsyncFunction("transactions") { productId: String, promise: Promise ->
      ready(promise) { billing ->
        owned(billing, promise) { purchases ->
          promise.resolve(purchases.filter {
            productId in it.products && it.purchaseState == Purchase.PurchaseState.PURCHASED
          }.map(::describe))
        }
      }
    }
    AsyncFunction("purchase") { productId: String, basePlanId: String, token: String, promise: Promise ->
      ready(promise) { billing ->
        if (purchase != null) {
          promise.reject("E_PURCHASE_BUSY", "Another purchase is already in progress.", null)
          return@ready
        }
        if (!token.matches(Regex("[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"))) {
          promise.reject("E_ACCOUNT", "Sign in before purchasing.", null)
          return@ready
        }
        details(billing, productId, basePlanId, promise) { product, offer ->
          val activity = appContext.currentActivity
          if (activity == null) {
            promise.reject("E_ACTIVITY", "Open the app to purchase.", null)
            return@details
          }
          val params = BillingFlowParams.newBuilder()
            .setObfuscatedAccountId(token)
            .setProductDetailsParamsList(listOf(
              BillingFlowParams.ProductDetailsParams.newBuilder()
                .setProductDetails(product).setOfferToken(offer.offerToken).build()
            )).build()
          purchase = promise
          purchaseProduct = productId
          purchaseTimeout = Runnable {
            rejectPurchase("E_PURCHASE_INTERRUPTED", "Purchase was interrupted. Restore purchases to retry.")
          }.also { main.postDelayed(it, 300_000) }
          val result = billing.launchBillingFlow(activity, params)
          when (result.responseCode) {
            BillingClient.BillingResponseCode.OK -> Unit
            BillingClient.BillingResponseCode.USER_CANCELED -> resolvePurchase(mapOf("status" to "cancelled"))
            BillingClient.BillingResponseCode.ITEM_ALREADY_OWNED -> owned(billing, promise) { current ->
              val existing = current.firstOrNull {
                productId in it.products && it.purchaseState == Purchase.PurchaseState.PURCHASED
              }
              if (existing != null) resolvePurchase(mapOf("status" to "purchased", "transaction" to describe(existing)))
              else rejectPurchase("E_ALREADY_OWNED", "Restore the existing subscription.")
            }
            else -> rejectPurchase("E_BILLING_${result.responseCode}", "Google Play could not start the purchase. Try again.")
          }
        }
      }
    }
  }

  private fun ready(promise: Promise, work: (BillingClient) -> Unit) {
    main.post {
      val current = client ?: run {
        val context = appContext.reactContext?.applicationContext
        if (context == null) {
          promise.reject("E_CONTEXT", "Billing is unavailable.", null)
          return@post
        }
        BillingClient.newBuilder(context).setListener(this)
          .enablePendingPurchases(PendingPurchasesParams.newBuilder().enableOneTimeProducts().build())
          .enableAutoServiceReconnection().build().also { client = it }
      }
      if (current.isReady) { work(current); return@post }
      waiting.add(promise to work)
      if (connecting) return@post
      connecting = true
      val timeout = Runnable { failConnection("Google Play connection timed out.") }
      main.postDelayed(timeout, 15_000)
      current.startConnection(object : BillingClientStateListener {
        override fun onBillingSetupFinished(result: BillingResult) {
          main.post {
            if (client !== current) return@post
            main.removeCallbacks(timeout)
            connecting = false
            val requests = waiting.toList(); waiting.clear()
            if (result.responseCode == BillingClient.BillingResponseCode.OK)
              requests.forEach { it.second(current) }
            else requests.forEach { it.first.reject("E_BILLING_${result.responseCode}", "Google Play billing is unavailable.", null) }
          }
        }
        override fun onBillingServiceDisconnected() { main.post { connecting = false } }
      })
    }
  }

  private fun failConnection(message: String) {
    connecting = false
    val requests = waiting.toList(); waiting.clear()
    requests.forEach { it.first.reject("E_BILLING_CONNECTION", message, null) }
    client?.endConnection(); client = null
  }

  private fun details(billing: BillingClient, productId: String, basePlanId: String,
                      promise: Promise, done: (ProductDetails, ProductDetails.SubscriptionOfferDetails) -> Unit) {
    val params = QueryProductDetailsParams.newBuilder().setProductList(listOf(
      QueryProductDetailsParams.Product.newBuilder().setProductId(productId)
        .setProductType(BillingClient.ProductType.SUBS).build()
    )).build()
    billing.queryProductDetailsAsync(params) { result, response -> main.post {
      val product = response.productDetailsList.firstOrNull { it.productId == productId }
      // Offer-free monthly base plan only: no trial, prepaid, instalment or alternate period.
      val offer = product?.subscriptionOfferDetails?.firstOrNull {
        MonthlyPlanPolicy.eligible(it.basePlanId, basePlanId, it.offerId != null,
          it.pricingPhases.pricingPhaseList.map { phase ->
            MonthlyPhase(phase.billingPeriod,
              phase.recurrenceMode == ProductDetails.RecurrenceMode.INFINITE_RECURRING,
              phase.priceAmountMicros)
          })
      }
      if (result.responseCode != BillingClient.BillingResponseCode.OK || product == null || offer == null)
        promise.reject("E_PRODUCT_UNAVAILABLE", "The monthly Google Play subscription is unavailable.", null)
      else done(product, offer)
    } }
  }

  private fun owned(billing: BillingClient, promise: Promise, done: (List<Purchase>) -> Unit) {
    val params = QueryPurchasesParams.newBuilder().setProductType(BillingClient.ProductType.SUBS)
      .includeSuspendedSubscriptions(true).build()
    billing.queryPurchasesAsync(params) { result, current -> main.post {
      if (result.responseCode == BillingClient.BillingResponseCode.OK) done(current)
      else promise.reject("E_RESTORE_FAILED", "Could not read Google Play purchases. Try again.", null)
    } }
  }

  override fun onPurchasesUpdated(result: BillingResult, purchases: List<Purchase>?) {
    main.post {
      when (result.responseCode) {
        BillingClient.BillingResponseCode.OK -> {
          for (item in purchases.orEmpty()) {
            if (item.purchaseState == Purchase.PurchaseState.PURCHASED) {
              if (observing) sendEvent("transaction", describe(item))
              if (purchaseProduct in item.products) resolvePurchase(mapOf("status" to "purchased", "transaction" to describe(item)))
            } else if (item.purchaseState == Purchase.PurchaseState.PENDING && purchaseProduct in item.products) {
              resolvePurchase(mapOf("status" to "pending"))
            }
          }
        }
        BillingClient.BillingResponseCode.USER_CANCELED -> resolvePurchase(mapOf("status" to "cancelled"))
        else -> rejectPurchase("E_BILLING_${result.responseCode}", "Google Play purchase failed. Try again or restore purchases.")
      }
    }
  }
  private fun describe(item: Purchase): Map<String, Any> = mapOf(
    "store" to "google_play",
    "id" to purchaseIdentity(item.purchaseToken),
    "productId" to item.products.first(), "purchaseToken" to item.purchaseToken
  )
  private fun resolvePurchase(value: Map<String, Any>) {
    purchaseTimeout?.let(main::removeCallbacks); purchaseTimeout = null
    val current = purchase; purchase = null; purchaseProduct = null
    current?.resolve(value)
  }
  private fun rejectPurchase(code: String, message: String) {
    purchaseTimeout?.let(main::removeCallbacks); purchaseTimeout = null
    val current = purchase; purchase = null; purchaseProduct = null
    current?.reject(code, message, null)
  }
}
