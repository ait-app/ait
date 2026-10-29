import ExpoModulesCore
import StoreKit

public final class AitStoreKitModule: Module {
  private var updates: Task<Void, Never>?
  private let productID = "monthly"

  public func definition() -> ModuleDefinition {
    Name("AitStoreKit")
    Events("transaction")

    OnStartObserving {
      self.updates?.cancel()
      self.updates = Task { [weak self] in
        for await result in Transaction.updates {
          guard !Task.isCancelled, let self else { break }
          if case .verified(let transaction) = result, transaction.productID == self.productID {
            self.sendEvent("transaction", self.describe(transaction))
          }
        }
      }
    }
    OnStopObserving { self.updates?.cancel(); self.updates = nil }
    OnDestroy { self.updates?.cancel(); self.updates = nil }

    AsyncFunction("product") { () async throws -> [String: Any]? in
      guard let product = try await Product.products(for: [self.productID]).first,
            product.type == .autoRenewable, let subscription = product.subscription,
            subscription.subscriptionPeriod.unit == .month,
            subscription.subscriptionPeriod.value == 1 else { return nil }
      return ["id": product.id, "displayPrice": product.displayPrice]
    }
    AsyncFunction("purchase") { (token: String) async throws -> [String: Any] in
      guard let accountToken = UUID(uuidString: token),
            let product = try await Product.products(for: [self.productID]).first,
            product.type == .autoRenewable else { throw BillingError.unavailable }
      switch try await product.purchase(options: [.appAccountToken(accountToken)]) {
      case .success(let result):
        guard case .verified(let transaction) = result else { throw BillingError.unverified }
        return ["status": "purchased", "transaction": self.describe(transaction)]
      case .pending: return ["status": "pending"]
      case .userCancelled: return ["status": "cancelled"]
      @unknown default: throw BillingError.unavailable
      }
    }
    AsyncFunction("transactions") { (includeHistory: Bool) async -> [[String: Any]] in
      var transactions: [UInt64: [String: Any]] = [:]
      for await result in Transaction.currentEntitlements {
        if case .verified(let transaction) = result, transaction.productID == self.productID {
          transactions[transaction.id] = self.describe(transaction)
        }
      }
      for await result in Transaction.unfinished {
        if case .verified(let transaction) = result, transaction.productID == self.productID {
          transactions[transaction.id] = self.describe(transaction)
        }
      }
      if includeHistory {
        for await result in Transaction.all {
          if case .verified(let transaction) = result, transaction.productID == self.productID {
            transactions[transaction.id] = self.describe(transaction)
          }
        }
      }
      return Array(transactions.values)
    }
    AsyncFunction("restore") { () async throws in try await AppStore.sync() }
    AsyncFunction("finish") { (id: String) async throws in
      for await result in Transaction.unfinished {
        if case .verified(let transaction) = result,
           String(transaction.id) == id, transaction.productID == self.productID {
          await transaction.finish()
          return
        }
      }
    }
  }

  private func describe(_ transaction: Transaction) -> [String: Any] {
    let environment: String
    switch transaction.environment {
    case .production: environment = "Production"
    case .sandbox: environment = "Sandbox"
    default: environment = "Xcode"
    }
    return ["id": String(transaction.id), "productId": transaction.productID,
            "environment": environment]
  }
}

private enum BillingError: Error {
  case unavailable
  case unverified
}
