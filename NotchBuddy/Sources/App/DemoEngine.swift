import Foundation
import SwiftUI

// MARK: - DemoEngine
// Drives a scripted ~60 s tour of Coucou's features.
// Nothing is written to disk, no network calls are made.

@MainActor
final class DemoEngine: ObservableObject {

    static let shared = DemoEngine()
    private init() {}

    // MARK: - Published state

    @Published var isActive: Bool = false

    // MARK: - Private state

    private var demoTask: Task<Void, Never>? = nil

    /// Continuations for interactive intercepts.
    private var approvalContinuation: CheckedContinuation<String, Never>? = nil
    private var questionContinuation: CheckedContinuation<Void, Never>? = nil

    // MARK: - AppState snapshot (restored on stop)

    private struct Snapshot {
        var tasks: [AgentTask]
        var focusId: String?
        var pendingApproval: ApprovalInfo?
        var pendingQuestion: AskQuestion?
        var chatHistory: [ChatMessage]
        var stateOverride: BotState?
        var vercelDeployments: [VercelDeployment]
        var resendEmails: [ResendEmail]
        var resendTotal: Int?
        var githubPulse: GitHubPulse?
        var githubActivity: GitHubActivity?
        var githubStats: GitHubStats?
        var stripePayments: [StripePayment]
        var stripeBalance: Int
        var stripeDisplayBalance: Int
        var stripeCurrency: String
        var stripeLoaded: Bool
        var stripeError: String?
        var calcomBookings: [CalcomBooking]
        var calcomLoaded: Bool
        var n8nRuns: [N8nRun]
        var notionPages: [NotionPage]
        var notionLoaded: Bool
        var claudePlanUsage: PlanUsage?
        var activeIntegrations: Set<String>
        var mode: IslandMode
        var view: IslandView
    }

    private var snapshot: Snapshot? = nil

    // MARK: - Start / Stop

    func start() {
        guard !isActive else { return }
        isActive = true
        let s = AppState.shared

        // Save snapshot
        snapshot = Snapshot(
            tasks:               s.tasks,
            focusId:             s.focusId,
            pendingApproval:     s.pendingApproval,
            pendingQuestion:     s.pendingQuestion,
            chatHistory:         s.chatHistory,
            stateOverride:       s.stateOverride,
            vercelDeployments:   s.vercelDeployments,
            resendEmails:        s.resendEmails,
            resendTotal:         s.resendTotal,
            githubPulse:         s.githubPulse,
            githubActivity:      s.githubActivity,
            githubStats:         s.githubStats,
            stripePayments:      s.stripePayments,
            stripeBalance:       s.stripeBalance,
            stripeDisplayBalance: s.stripeDisplayBalance,
            stripeCurrency:      s.stripeCurrency,
            stripeLoaded:        s.stripeLoaded,
            stripeError:         s.stripeError,
            calcomBookings:      s.calcomBookings,
            calcomLoaded:        s.calcomLoaded,
            n8nRuns:             s.n8nRuns,
            notionPages:         s.notionPages,
            notionLoaded:        s.notionLoaded,
            claudePlanUsage:     s.claudePlanUsage,
            activeIntegrations:  s.activeIntegrations,
            mode:                s.mode,
            view:                s.view
        )

        // Inject demo integration data
        injectIntegrationData()

        // Launch loop
        demoTask = Task { @MainActor in
            await runDemoLoop()
        }
    }

    func stop() {
        guard isActive else { return }
        isActive = false
        demoTask?.cancel()
        demoTask = nil

        // Resume any blocked continuations so the task can exit cleanly
        let ac = approvalContinuation
        approvalContinuation = nil
        ac?.resume(returning: "allow")

        let qc = questionContinuation
        questionContinuation = nil
        qc?.resume()

        // Clear demo summary override
        RecapStore.shared.demoSummaryOverride = nil

        // Restore snapshot
        guard let snap = snapshot else { return }
        snapshot = nil
        let s = AppState.shared

        s.tasks               = snap.tasks
        s.focusId             = snap.focusId
        s.pendingApproval     = snap.pendingApproval
        s.pendingQuestion     = snap.pendingQuestion
        s.chatHistory         = snap.chatHistory
        s.stateOverride       = snap.stateOverride
        s.vercelDeployments   = snap.vercelDeployments
        s.resendEmails        = snap.resendEmails
        s.resendTotal         = snap.resendTotal
        s.githubPulse         = snap.githubPulse
        s.githubActivity      = snap.githubActivity
        s.githubStats         = snap.githubStats
        s.stripePayments      = snap.stripePayments
        s.stripeBalance       = snap.stripeBalance
        s.stripeDisplayBalance = snap.stripeDisplayBalance
        s.stripeCurrency      = snap.stripeCurrency
        s.stripeLoaded        = snap.stripeLoaded
        s.stripeError         = snap.stripeError
        s.calcomBookings      = snap.calcomBookings
        s.calcomLoaded        = snap.calcomLoaded
        s.n8nRuns             = snap.n8nRuns
        s.notionPages         = snap.notionPages
        s.notionLoaded        = snap.notionLoaded
        s.claudePlanUsage     = snap.claudePlanUsage
        s.activeIntegrations  = snap.activeIntegrations
        s.mode                = snap.mode
        s.view                = snap.view

        NotificationCenter.default.post(name: .islandCollapse, object: nil)
    }

    // MARK: - Integration data injection

    private func injectIntegrationData() {
        let s = AppState.shared
        let now = Date()

        // GitHub
        s.githubStats = GitHubStats(totalRepos: 34, totalStars: 127)
        s.githubPulse = GitHubPulse(
            login: "demo-user",
            myPRs: [
                GitHubPR(id: "my-app/auth#42", title: "feat: OAuth2 PKCE flow", url: "https://github.com/demo/my-app/pull/42",
                         repo: "my-app", number: 42, isDraft: false, ci: .success, review: .approved),
                GitHubPR(id: "my-app/api#38", title: "fix: rate limit headers", url: "https://github.com/demo/my-app/pull/38",
                         repo: "my-app", number: 38, isDraft: false, ci: .pending, review: .pending),
            ],
            toReview: [
                GitHubPR(id: "my-app/ui#21", title: "refactor: design system tokens", url: "https://github.com/demo/my-app/pull/21",
                         repo: "my-app", number: 21, isDraft: false, ci: .success, review: .pending),
            ],
            mainCI: [
                GitHubRepoCI(repo: "my-app", url: "https://github.com/demo/my-app/actions", branch: "main", ci: .success),
            ],
            fetchedAt: now
        )
        s.githubActivity = nil

        // Stripe
        s.stripeCurrency       = "eur"
        s.stripeBalance        = 124750   // €1,247.50
        s.stripeDisplayBalance = 124750
        s.stripeLoaded         = true
        s.stripeError          = nil
        s.stripePayments = [
            StripePayment(id: "py_demo1", amount: 4900, currency: "eur",
                          description: "Pro plan — monthly", createdAt: now.addingTimeInterval(-3600),
                          status: "succeeded"),
            StripePayment(id: "py_demo2", amount: 9900, currency: "eur",
                          description: "Pro plan — annual", createdAt: now.addingTimeInterval(-7200),
                          status: "succeeded"),
            StripePayment(id: "py_demo3", amount: 2900, currency: "eur",
                          description: "Starter plan", createdAt: now.addingTimeInterval(-14400),
                          status: "succeeded"),
        ]

        // Vercel
        s.vercelDeployments = [
            VercelDeployment(id: "dpl_demo1", projectName: "my-app",
                             url: "my-app-abc123.vercel.app", state: "READY",
                             createdAt: now.addingTimeInterval(-1800),
                             commitMessage: "feat: auth refactor", branch: "main"),
            VercelDeployment(id: "dpl_demo2", projectName: "my-app",
                             url: "my-app-prev.vercel.app", state: "READY",
                             createdAt: now.addingTimeInterval(-86400),
                             commitMessage: "fix: mobile nav", branch: "main"),
        ]

        // Resend
        s.resendTotal = 1284
        s.resendEmails = [
            ResendEmail(id: "re_demo1", to: ["alice@example.com"], subject: "Welcome to my-app",
                        createdAt: now.addingTimeInterval(-600), lastEvent: "delivered"),
            ResendEmail(id: "re_demo2", to: ["bob@example.com"], subject: "Your weekly digest",
                        createdAt: now.addingTimeInterval(-3600), lastEvent: "opened"),
        ]

        // Cal.com — 2 bookings tomorrow
        let tomorrow = Calendar.current.date(byAdding: .day, value: 1, to: now)!
        let cal      = Calendar.current
        let tomorrowMorning = cal.date(bySettingHour: 10, minute: 0, second: 0, of: tomorrow)!
        let tomorrowNoon    = cal.date(bySettingHour: 14, minute: 30, second: 0, of: tomorrow)!
        s.calcomBookings = [
            CalcomBooking(id: 1001, title: "Product demo call",
                          startTime: tomorrowMorning,
                          endTime: tomorrowMorning.addingTimeInterval(3600),
                          status: "ACCEPTED",
                          attendeeName: "Alice Martin",
                          attendeeEmail: "alice@example.com",
                          attendeeNotes: "Interested in the Pro plan"),
            CalcomBooking(id: 1002, title: "Onboarding call",
                          startTime: tomorrowNoon,
                          endTime: tomorrowNoon.addingTimeInterval(1800),
                          status: "ACCEPTED",
                          attendeeName: "Bob Chen",
                          attendeeEmail: "bob@example.com",
                          attendeeNotes: nil),
        ]
        s.calcomLoaded = true

        // n8n
        s.n8nRuns = [
            N8nRun(workflow: "Notify on new Stripe payment", detail: "3 payments processed",
                   success: true, date: now.addingTimeInterval(-300)),
        ]

        // Notion
        s.notionPages = [
            NotionPage(id: "notion_demo1", title: "Product Roadmap Q4",
                       emoji: "🗺️", lastEditedAt: now.addingTimeInterval(-1800),
                       url: "https://notion.so/demo/roadmap"),
            NotionPage(id: "notion_demo2", title: "Auth Refactor Notes",
                       emoji: "🔐", lastEditedAt: now.addingTimeInterval(-7200),
                       url: "https://notion.so/demo/auth-notes"),
        ]
        s.notionLoaded = true

        // Claude plan usage
        s.claudePlanUsage = PlanUsage(
            fiveHour: PlanWindow(usedPct: 42.0, resetsAt: now.addingTimeInterval(4 * 3600)),
            sevenDay: PlanWindow(usedPct: 37.0, resetsAt: now.addingTimeInterval(3 * 24 * 3600)),
            updatedAt: now
        )

        // Force demo integrations active (max 4)
        s.activeIntegrations = ["integration_github", "integration_stripe", "integration_vercel", "integration_resend"]
        s.loadIntegrationTasks()
    }

    // MARK: - Demo loop

    private func runDemoLoop() async {
        while !Task.isCancelled {
            await runOneDemoCycle()
            guard !Task.isCancelled else { break }
            // Brief pause between cycles
            try? await Task.sleep(nanoseconds: 3_000_000_000)
        }
    }

    private func runOneDemoCycle() async {
        guard isActive else { return }
        let s = AppState.shared
        let mainPillId = s.mainPillId

        // ── Step 1: Reveal island compact ──────────────────────────────────────
        NotificationCenter.default.post(name: .hookReveal, object: nil)
        await sleep(1.5)
        guard isActive else { return }

        // ── Step 2: Start VS Code session with steps ────────────────────────
        s.updateTask(id: mainPillId, state: .working)
        s.focusId = mainPillId
        if let idx = s.tasks.firstIndex(where: { $0.id == mainPillId }) {
            s.tasks[idx].steps = []
            s.tasks[idx].stepIndex = 0
            s.tasks[idx].finalLine = nil
        }

        let steps = [
            "Reading auth/middleware.ts",
            "Editing components/LoginForm.tsx (+32 -8)",
            "Running npm test",
        ]

        for (i, step) in steps.enumerated() {
            await pauseIfHidden()
            guard isActive else { return }
            if let idx = s.tasks.firstIndex(where: { $0.id == mainPillId }) {
                s.tasks[idx].steps.append(step)
                s.tasks[idx].stepIndex = i
            }
            // After the second step (index 1), append the real FileDiff
            if i == 1 {
                let oldCode = """
                const handleSubmit = async (e) => {
                  e.preventDefault()
                  setLoading(true)
                  const result = await signIn(email, password)
                  router.push('/dashboard')
                  setLoading(false)
                }
                """
                let newCode = """
                const handleSubmit = useCallback(async (e: FormEvent) => {
                  e.preventDefault()
                  setLoading(true)
                  try {
                    const result = await signIn(email, password)
                    if (result.error) throw new Error(result.error)
                    router.push('/dashboard')
                  } catch (err) {
                    setError((err as Error).message)
                  } finally {
                    setLoading(false)
                  }
                }, [email, password, router])
                """
                let diff = DiffEngine.fromEdit(old: oldCode, new: newCode, path: "components/LoginForm.tsx")
                s.appendSessionDiff(diff, for: mainPillId)
            }
            await sleep(2.0)
            guard isActive else { return }
        }

        // ── Step 3: Start second session (Codex) ────────────────────────────
        await pauseIfHidden()
        guard isActive else { return }
        let codexTask = AgentTask(
            id: "demo_codex",
            name: "Codex",
            color: "#E07950",
            state: .working,
            steps: ["Reading src/api/routes.ts", "Editing src/api/routes.ts (+15 -3)"],
            source: .agent,
            isIntegration: false
        )
        if !s.tasks.contains(where: { $0.id == "demo_codex" }) {
            s.tasks.append(codexTask)
            s.syncMode()
        }
        await sleep(1.5)
        guard isActive else { return }

        // ── Step 4: Permission request ───────────────────────────────────────
        await pauseIfHidden()
        guard isActive else { return }
        SoundEngine.shared.play("approval")
        s.pendingApproval = ApprovalInfo(
            sessionId: "demo_session",
            tool: "Bash",
            command: "npm test",
            inputKey: #"{"command":"npm test"}"#,
            pillId: mainPillId
        )
        s.isPinned = true
        NotificationCenter.default.post(name: .hookExpand, object: IslandView.approval)

        let _ = await waitForApprovalOrTimeout(seconds: 8.0)
        guard isActive else { return }

        // ── Step 5: AskUserQuestion ──────────────────────────────────────────
        await pauseIfHidden()
        guard isActive else { return }
        await sleep(0.5)
        SoundEngine.shared.play("question")
        s.pendingQuestion = AskQuestion(questions: [
            AskQuestionItem(
                question: "Which test reporter format?",
                header: "Reporter",
                options: [
                    AskQuestionOption(label: "Verbose", description: "Full output for each test"),
                    AskQuestionOption(label: "Dot",     description: "Minimal one-dot-per-test"),
                    AskQuestionOption(label: "JSON",    description: "Machine-readable JSON report"),
                ],
                multiSelect: false
            )
        ])
        s.isPinned = true
        NotificationCenter.default.post(name: .hookExpand, object: IslandView.question)

        await waitForQuestionOrTimeout(seconds: 8.0)
        guard isActive else { return }

        // ── Step 6: Finish primary session ──────────────────────────────────
        await pauseIfHidden()
        guard isActive else { return }
        await sleep(0.5)
        SoundEngine.shared.play("finish")
        if let idx = s.tasks.firstIndex(where: { $0.id == mainPillId }) {
            s.tasks[idx].state     = .finished
            s.tasks[idx].finalLine = "All 23 tests pass. Auth refactor complete — 94 % coverage."
        }
        NotificationCenter.default.post(name: .hookExpand, object: IslandView.finished)
        await sleep(2.5)
        guard isActive else { return }

        // ── Step 7: Chat ─────────────────────────────────────────────────────
        await pauseIfHidden()
        guard isActive else { return }
        s.chatHistory = []
        NotificationCenter.default.post(name: .hookExpand, object: IslandView.prompt)
        await sleep(1.0)
        guard isActive else { return }

        // Add user message
        s.chatHistory.append(ChatMessage(role: .user, content: "What did you change in LoginForm?"))
        await sleep(0.4)
        guard isActive else { return }

        // Stream fake response word by word
        await streamChatResponse(for: "What did you change in LoginForm?")
        guard isActive else { return }
        await sleep(2.0)

        // ── Step 8: Weekly recap ─────────────────────────────────────────────
        await pauseIfHidden()
        guard isActive else { return }
        RecapStore.shared.demoSummaryOverride = demoWeeklySummary()
        NotificationCenter.default.post(name: .hookExpand, object: IslandView.recap)
        await sleep(5.0)
        guard isActive else { return }

        // ── Step 9: Reset for next loop ───────────────────────────────────────
        RecapStore.shared.demoSummaryOverride = nil
        s.clearSessionDiffs(for: mainPillId)
        s.chatHistory = []
        s.stateOverride = nil
        if let idx = s.tasks.firstIndex(where: { $0.id == mainPillId }) {
            s.tasks[idx].state     = .idle
            s.tasks[idx].steps     = []
            s.tasks[idx].stepIndex = 0
            s.tasks[idx].finalLine = nil
            s.tasks[idx].pillBadge = nil
        }
        s.tasks.removeAll { $0.id == "demo_codex" }
        s.pendingApproval = nil
        s.pendingQuestion = nil
        s.isPinned = false
        s.focusId = mainPillId
        NotificationCenter.default.post(name: .islandCollapse, object: nil)
    }

    // MARK: - Intercept handlers (called by HookServer / ClaudeService)

    func handleApprovalDecision(_ decision: String) {
        let c = approvalContinuation
        approvalContinuation = nil
        c?.resume(returning: decision)
    }

    func handleQuestionAnswered() {
        let c = questionContinuation
        questionContinuation = nil
        c?.resume()
    }

    func streamChatResponse(for query: String) async {
        let s = AppState.shared
        let response = """
        I refactored LoginForm.tsx to be type-safe and resilient. \
        The main change is wrapping handleSubmit in a useCallback so it only recreates \
        when its dependencies change, and adding a proper try/catch/finally block so \
        loading is always reset even on error. An error state now shows the message \
        inline rather than letting the exception bubble up unhandled. \
        Coverage went from 79 % to 94 % after the test suite caught two edge \
        cases the old code missed.
        """

        // Add empty assistant message to stream into
        let msg = ChatMessage(role: .assistant, content: "")
        s.chatHistory.append(msg)
        guard let msgIdx = s.chatHistory.lastIndex(where: { $0.role == .assistant && $0.content == "" }) else { return }

        let words = response.components(separatedBy: " ")
        var built = ""
        for word in words {
            guard isActive, !Task.isCancelled else { break }
            built += (built.isEmpty ? "" : " ") + word
            s.chatHistory[msgIdx].content = built
            try? await Task.sleep(nanoseconds: 60_000_000)  // 60ms per word
        }
    }

    // MARK: - Helpers

    /// Pauses (in 200ms loops) while the island is hidden and demo is active.
    private func pauseIfHidden() async {
        while isActive && AppState.shared.mode == .hidden {
            try? await Task.sleep(nanoseconds: 200_000_000)
        }
    }

    /// Waits for user to click approval, or auto-resumes with "allow" after timeout.
    private func waitForApprovalOrTimeout(seconds: Double) async -> String {
        return await withCheckedContinuation { continuation in
            self.approvalContinuation = continuation
            let ns = UInt64(seconds * 1_000_000_000)
            Task { @MainActor in
                try? await Task.sleep(nanoseconds: ns)
                guard self.isActive else { return }
                let c = self.approvalContinuation
                guard c != nil else { return }  // already resumed by user click
                self.approvalContinuation = nil
                // Auto-allow: clear the card ourselves then resume
                let s = AppState.shared
                s.pendingApproval = nil
                s.isPinned = false
                s.view = s.tasks.isEmpty ? .empty : .overview
                c?.resume(returning: "allow")
            }
        }
    }

    /// Waits for user to answer the question, or auto-resumes after timeout.
    private func waitForQuestionOrTimeout(seconds: Double) async {
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            self.questionContinuation = continuation
            let ns = UInt64(seconds * 1_000_000_000)
            Task { @MainActor in
                try? await Task.sleep(nanoseconds: ns)
                guard self.isActive else { return }
                let c = self.questionContinuation
                guard c != nil else { return }  // already resumed by user
                self.questionContinuation = nil
                let s = AppState.shared
                s.pendingQuestion = nil
                s.isPinned = false
                s.view = s.tasks.isEmpty ? .empty : .overview
                c?.resume()
            }
        }
    }

    /// Thin wrapper to keep callsites readable.
    private func sleep(_ seconds: Double) async {
        try? await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
    }

    // MARK: - Demo weekly summary

    private func demoWeeklySummary() -> WeeklySummary {
        let cal = Calendar(identifier: .iso8601)
        var comps = cal.dateComponents([.yearForWeekOfYear, .weekOfYear], from: Date())
        comps.weekday = 2
        let thisMonday = cal.date(from: comps) ?? Date()
        let lastMonday = cal.date(byAdding: .weekOfYear, value: -1, to: thisMonday) ?? Date()
        let lastSunday = cal.date(byAdding: .day, value: 6, to: lastMonday) ?? Date()

        return WeeklySummary(
            weekStart:             lastMonday,
            weekEnd:               lastSunday,
            totalMinutes:          840,   // 14 h
            sessionCount:          23,
            filesChanged:          187,
            linesAdded:            3412,
            linesRemoved:          891,
            commandsRun:           142,
            questionsAnswered:     31,
            permissionsAllowed:    58,
            permissionsDenied:     4,
            topAgent:              "Claude Code",
            topProject:            "my-app",
            busiestDay:            "Wednesday",
            longestSessionMinutes: 94
        )
    }
}
