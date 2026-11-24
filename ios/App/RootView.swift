import BoxCore
import SwiftUI

/// Pages swiped sideways, with dots at the bottom: the native race screen first, then the full
/// web dashboard of the server (strategy, practice, coach, recap…) until those are native too.
struct RootView: View {
    @SceneStorage("page") private var page = 0

    var body: some View {
        TabView(selection: $page) {
            RaceView().tag(0)
            DashboardPage().tag(1)
        }
        .tabViewStyle(.page(indexDisplayMode: .always))
        .indexViewStyle(.page(backgroundDisplayMode: .interactive))
        .background(Theme.bg.ignoresSafeArea())
        .ignoresSafeArea(edges: .bottom)
    }
}
