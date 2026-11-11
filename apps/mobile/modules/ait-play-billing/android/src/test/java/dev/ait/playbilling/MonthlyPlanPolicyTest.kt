package dev.ait.playbilling

import org.junit.Assert.*
import org.junit.Test

class MonthlyPlanPolicyTest {
  @Test fun acceptsOnlyTheConfiguredPaidMonthlyBasePlan() {
    val monthly = MonthlyPhase("P1M", true, 4_990_000)
    assertTrue(MonthlyPlanPolicy.eligible("monthly", "monthly", false, listOf(monthly)))
    assertFalse(MonthlyPlanPolicy.eligible("annual", "monthly", false, listOf(monthly)))
    assertFalse(MonthlyPlanPolicy.eligible("monthly", "monthly", true, listOf(monthly)))
    for (phase in listOf(MonthlyPhase("P1Y", true, 4_990_000),
      MonthlyPhase("P1M", false, 4_990_000), MonthlyPhase("P1M", true, 0))) {
      assertFalse(MonthlyPlanPolicy.eligible("monthly", "monthly", false, listOf(phase)))
    }
    assertFalse(MonthlyPlanPolicy.eligible("monthly", "monthly", false, emptyList()))
    assertFalse(MonthlyPlanPolicy.eligible("monthly", "monthly", false, listOf(monthly, monthly)))
  }
  @Test fun hashesPurchaseTokensForStableNonSecretDeliveryIds() {
    val first = purchaseIdentity("purchase-fixture-a")
    assertEquals(64, first.length)
    assertEquals(first, purchaseIdentity("purchase-fixture-a"))
    assertNotEquals(first, purchaseIdentity("purchase-fixture-b"))
    assertFalse(first.contains("purchase-fixture"))
  }
}
