import UIKit

// Expo 54 only dispatches application-delegate callbacks. Bridge the single scene to those
// callbacks until an Expo upgrade supplies its own scene delegate. Do not post application
// notifications here: UIKit still sends them, and React Native AppState observes them.
final class AitSceneDelegate: UIResponder, UIWindowSceneDelegate {
  var window: UIWindow?

  private var appDelegate: AppDelegate? {
    UIApplication.shared.delegate as? AppDelegate
  }

  func scene(
    _ scene: UIScene,
    willConnectTo session: UISceneSession,
    options connectionOptions: UIScene.ConnectionOptions
  ) {
    guard let windowScene = scene as? UIWindowScene, let appDelegate else { return }

    if let existingWindow = appDelegate.window {
      // A disconnected scene can reconnect without creating another React Native host.
      window = existingWindow
      existingWindow.windowScene = windowScene
      existingWindow.makeKeyAndVisible()
      forward(connectionOptions)
      return
    }

    var launchOptions = appDelegate.initialLaunchOptions ?? [:]
    if let context = connectionOptions.urlContexts.first {
      launchOptions[.url] = context.url
      launchOptions[.sourceApplication] = context.options.sourceApplication
    }
    if let activity = connectionOptions.userActivities.first(where: {
      $0.activityType == NSUserActivityTypeBrowsingWeb
    }) {
      launchOptions[.userActivityDictionary] = [
        "UIApplicationLaunchOptionsUserActivityTypeKey": activity.activityType,
        "UIApplicationLaunchOptionsUserActivityKey": activity,
      ]
    }
    if let response = connectionOptions.notificationResponse {
      launchOptions[.remoteNotification] = response.notification.request.content.userInfo
    }

    // Seed expo-linking before JS subscribes; launchOptions independently supplies React
    // Native Linking.getInitialURL(). Warm links use the same existing delegate handlers.
    forward(connectionOptions)
    let sceneWindow = UIWindow(windowScene: windowScene)
    window = sceneWindow
    appDelegate.window = sceneWindow
    appDelegate.startReactNativeForScene(in: sceneWindow, launchOptions: launchOptions)
    appDelegate.initialLaunchOptions = nil
  }

  private func forward(_ options: UIScene.ConnectionOptions) {
    open(options.urlContexts)
    for activity in options.userActivities {
      continueActivity(activity)
    }
  }

  func scene(_ scene: UIScene, openURLContexts urlContexts: Set<UIOpenURLContext>) {
    open(urlContexts)
  }

  private func open(_ contexts: Set<UIOpenURLContext>) {
    for context in contexts {
      var options: [UIApplication.OpenURLOptionsKey: Any] = [
        .openInPlace: context.options.openInPlace
      ]
      options[.sourceApplication] = context.options.sourceApplication
      options[.annotation] = context.options.annotation
      _ = appDelegate?.application(UIApplication.shared, open: context.url, options: options)
    }
  }

  func scene(_ scene: UIScene, continue userActivity: NSUserActivity) {
    continueActivity(userActivity)
  }

  private func continueActivity(_ activity: NSUserActivity) {
    _ = appDelegate?.application(
      UIApplication.shared, continue: activity, restorationHandler: { _ in })
  }

  func sceneDidBecomeActive(_ scene: UIScene) {
    appDelegate?.applicationDidBecomeActive(UIApplication.shared)
  }

  func sceneWillResignActive(_ scene: UIScene) {
    appDelegate?.applicationWillResignActive(UIApplication.shared)
  }

  func sceneDidEnterBackground(_ scene: UIScene) {
    appDelegate?.applicationDidEnterBackground(UIApplication.shared)
  }

  func sceneWillEnterForeground(_ scene: UIScene) {
    appDelegate?.applicationWillEnterForeground(UIApplication.shared)
  }
}
