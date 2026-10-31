package dev.ait.playbilling

import java.security.MessageDigest

internal data class MonthlyPhase(val period: String, val recurring: Boolean, val priceMicros: Long)

/** Offer-free, positive-price monthly renewal is the only product this app sells. */
internal object MonthlyPlanPolicy {
  fun eligible(actualBase: String, expectedBase: String, hasOffer: Boolean,
               phases: List<MonthlyPhase>): Boolean {
    val phase = phases.singleOrNull() ?: return false
    return actualBase == expectedBase && !hasOffer && phase.period == "P1M" &&
      phase.recurring && phase.priceMicros > 0
  }
}

/** Redelivery identity is stable without using the secret purchase token as a public ID. */
internal fun purchaseIdentity(token: String): String =
  MessageDigest.getInstance("SHA-256").digest(token.toByteArray())
    .joinToString("") { "%02x".format(it.toInt() and 255) }
