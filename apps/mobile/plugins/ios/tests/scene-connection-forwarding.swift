// Platform doubles for the generated scene connection callback. The production method is
// inserted below, so dropping a notification response fails the same harness as losing a link.
struct Response: Equatable {
  let agentId: String
}

enum UIScene {
  struct ConnectionOptions {
    var notificationResponse: Response?
    var urlContexts: Set<String> = []
    var userActivities: [String] = []
  }
}

struct UNUserNotificationCenter {
  static func current() -> Self { Self() }
}

final class NotificationCenterManager {
  static let shared = NotificationCenterManager()
  var responses: [Response] = []

  func userNotificationCenter(
    _ center: UNUserNotificationCenter,
    didReceive response: Response,
    withCompletionHandler completionHandler: () -> Void
  ) {
    responses.append(response)
    completionHandler()
  }
}

final class Scene {
  var urls: Set<String> = []
  var activities: [String] = []

  func connect(_ options: UIScene.ConnectionOptions) { forward(options) }
  private func open(_ contexts: Set<String>) { urls.formUnion(contexts) }
  private func continueActivity(_ activity: String) { activities.append(activity) }

  // FORWARD_METHOD
}

let coldResponse = Response(agentId: "cold-session")
let reconnectResponse = Response(agentId: "reconnected-session")
let scene = Scene()
scene.connect(.init(notificationResponse: coldResponse, urlContexts: ["ait://workspace"]))
precondition(NotificationCenterManager.shared.responses == [coldResponse])
precondition(scene.urls == ["ait://workspace"])

scene.connect(.init(notificationResponse: reconnectResponse, userActivities: ["universal-link"]))
precondition(NotificationCenterManager.shared.responses == [coldResponse, reconnectResponse])
precondition(scene.activities == ["universal-link"])

scene.connect(.init(notificationResponse: nil))
precondition(NotificationCenterManager.shared.responses == [coldResponse, reconnectResponse])
