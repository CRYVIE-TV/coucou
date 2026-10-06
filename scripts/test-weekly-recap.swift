#!/usr/bin/env swift
// test-weekly-recap.swift
// Fills recap.json with one fake week of activity so the weekly recap card has something to show.
// Also renders RecapShareImageView to a PNG at ~/Desktop/coucou-recap-preview.png.
// Usage:  swift scripts/test-weekly-recap.swift
// After running, open Coucou and choose "Weekly recap" from the menu bar, or wait for Monday ≥ 8 am.

import Foundation
import SwiftUI
import AppKit

// MARK: - Mirror the Codable models from RecapStore.swift

struct RecapTurn: Codable {
    var pillId: String
    var project: String
    var start: Date
    var end: Date
    var filesChanged: Int
    var linesAdded: Int
    var linesRemoved: Int
    var commandsRun: Int
    var questions: Int
}

struct RecapDecision: Codable {
    var pillId: String
    var date: Date
    var decision: String
}

struct RecapData: Codable {
    var turns: [RecapTurn]
    var decisions: [RecapDecision]
    var schemaVersion: Int
}

// MARK: - Target path

let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
let recapURL = support.appendingPathComponent("NotchBuddy/recap.json")

// Load existing data (if any) so we don't clobber other weeks.
var data: RecapData
if let raw = try? Data(contentsOf: recapURL),
   let decoded = try? JSONDecoder().decode(RecapData.self, from: raw) {
    data = decoded
    print("Loaded \(data.turns.count) existing turns.")
} else {
    data = RecapData(turns: [], decisions: [], schemaVersion: 1)
    print("Starting fresh.")
}

// MARK: - Fake data: one week ago Mon–Sun

let cal = Calendar.current
var comps = cal.dateComponents([.yearForWeekOfYear, .weekOfYear], from: Date())
comps.weekday = 2
let thisMonday = cal.date(from: comps)!
let lastMonday = cal.date(byAdding: .weekOfYear, value: -1, to: thisMonday)!

func makeDate(dayOffset: Int, hour: Int, minute: Int = 0) -> Date {
    cal.date(byAdding: .second, value: dayOffset * 86400 + hour * 3600 + minute * 60, to: lastMonday)!
}

let fakeTurns: [RecapTurn] = [
    // Monday: long Claude Code session
    RecapTurn(pillId: "integration_claude", project: "coucou",
              start: makeDate(dayOffset: 0, hour: 9),
              end:   makeDate(dayOffset: 0, hour: 11, minute: 30),
              filesChanged: 8, linesAdded: 312, linesRemoved: 87,
              commandsRun: 14, questions: 2),
    // Monday: afternoon session
    RecapTurn(pillId: "integration_claude", project: "coucou",
              start: makeDate(dayOffset: 0, hour: 14),
              end:   makeDate(dayOffset: 0, hour: 15, minute: 45),
              filesChanged: 3, linesAdded: 95, linesRemoved: 20,
              commandsRun: 5, questions: 0),
    // Tuesday: Gemini CLI
    RecapTurn(pillId: "agent_gemini", project: "side-project",
              start: makeDate(dayOffset: 1, hour: 10),
              end:   makeDate(dayOffset: 1, hour: 11),
              filesChanged: 2, linesAdded: 50, linesRemoved: 10,
              commandsRun: 3, questions: 1),
    // Wednesday: Claude Code
    RecapTurn(pillId: "integration_claude", project: "coucou",
              start: makeDate(dayOffset: 2, hour: 9, minute: 30),
              end:   makeDate(dayOffset: 2, hour: 12),
              filesChanged: 5, linesAdded: 180, linesRemoved: 60,
              commandsRun: 8, questions: 3),
    // Thursday: short burst
    RecapTurn(pillId: "integration_claude", project: "coucou",
              start: makeDate(dayOffset: 3, hour: 16),
              end:   makeDate(dayOffset: 3, hour: 17),
              filesChanged: 1, linesAdded: 40, linesRemoved: 5,
              commandsRun: 2, questions: 0),
    // Friday: longest session
    RecapTurn(pillId: "integration_claude", project: "coucou",
              start: makeDate(dayOffset: 4, hour: 8),
              end:   makeDate(dayOffset: 4, hour: 13),
              filesChanged: 12, linesAdded: 540, linesRemoved: 130,
              commandsRun: 22, questions: 5),
]

let fakeDecisions: [RecapDecision] = [
    RecapDecision(pillId: "integration_claude", date: makeDate(dayOffset: 0, hour: 9, minute: 30), decision: "allow"),
    RecapDecision(pillId: "integration_claude", date: makeDate(dayOffset: 0, hour: 10), decision: "allow"),
    RecapDecision(pillId: "integration_claude", date: makeDate(dayOffset: 2, hour: 10), decision: "deny"),
    RecapDecision(pillId: "integration_claude", date: makeDate(dayOffset: 4, hour: 9), decision: "always"),
    RecapDecision(pillId: "integration_claude", date: makeDate(dayOffset: 4, hour: 10), decision: "allow"),
]

// Remove any existing turns for the same week to avoid duplicates
let weekEnd = cal.date(byAdding: .day, value: 7, to: lastMonday)!
data.turns = data.turns.filter { $0.start < lastMonday || $0.start >= weekEnd }
data.decisions = data.decisions.filter { $0.date < lastMonday || $0.date >= weekEnd }

data.turns += fakeTurns
data.decisions += fakeDecisions

// MARK: - Write

let encoder = JSONEncoder()
encoder.outputFormatting = .prettyPrinted

try FileManager.default.createDirectory(at: recapURL.deletingLastPathComponent(),
                                         withIntermediateDirectories: true)
let encoded = try encoder.encode(data)
try encoded.write(to: recapURL, options: .atomic)

print("Wrote \(fakeTurns.count) test turns to \(recapURL.path)")
print("Total activity: ~\(fakeTurns.reduce(0) { $0 + Int($1.end.timeIntervalSince($1.start) / 60) }) minutes")
print("")
print("Open Coucou → menu bar → 'Weekly recap' to see the card.")
print("Or run `open \(recapURL.deletingLastPathComponent().path)` to inspect the file.")

// MARK: - Render share card to PNG

// Types and helpers for the standalone share-card renderer (no BotEngine dependency).

struct PreviewSummary {
    var weekStart: Date; var weekEnd: Date; var totalMinutes: Int; var sessionCount: Int
    var filesChanged: Int; var linesAdded: Int; var linesRemoved: Int; var commandsRun: Int
    var questionsAnswered: Int; var permissionsAllowed: Int; var permissionsDenied: Int
    var topAgent: String?; var topProject: String?; var busiestDay: String?
    var longestSessionMinutes: Int
}

extension Color {
    init(hex: String) {
        let h = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var int: UInt64 = 0; Scanner(string: h).scanHexInt64(&int)
        self.init(red:   Double((int >> 16) & 0xFF) / 255,
                  green: Double((int >>  8) & 0xFF) / 255,
                  blue:  Double( int        & 0xFF) / 255)
    }
}

struct PreviewShareImageView: View {
    let summary: PreviewSummary
    var body: some View {
        ZStack {
            Color(hex: "#0B0C0E")
            VStack(spacing: 0) {
                Spacer()
                Text("Coucou")
                    .font(.system(size: 52, weight: .black, design: .rounded))
                    .foregroundColor(Color(hex: "#F1F2F4"))
                Text("Weekly recap")
                    .font(.system(size: 30, weight: .medium))
                    .foregroundColor(Color(hex: "#8E939C")).padding(.top, 6)
                Text(weekLabel)
                    .font(.system(size: 24)).foregroundColor(Color(hex: "#818CF8"))
                    .padding(.top, 14).padding(.bottom, 80)
                Text(fmtDur(summary.totalMinutes))
                    .font(.system(size: 100, weight: .black, design: .rounded))
                    .foregroundColor(Color(hex: "#F1F2F4")).minimumScaleFactor(0.4).lineLimit(1)
                Text("TIME CODING")
                    .font(.system(size: 22, weight: .semibold))
                    .foregroundColor(Color(hex: "#8E939C")).tracking(3)
                HStack(spacing: 0) {
                    statBlock("\(summary.sessionCount)", label: "SESSIONS")
                    Rectangle().fill(Color.white.opacity(0.08)).frame(width: 1, height: 80)
                    statBlock("\(summary.filesChanged)", label: "FILES")
                    if summary.commandsRun > 0 {
                        Rectangle().fill(Color.white.opacity(0.08)).frame(width: 1, height: 80)
                        statBlock("\(summary.commandsRun)", label: "COMMANDS")
                    }
                }
                .padding(.top, 56).padding(.horizontal, 40)
                if summary.linesAdded + summary.linesRemoved > 0 {
                    HStack(spacing: 24) {
                        Text("+\(summary.linesAdded)")
                            .font(.system(size: 28, weight: .semibold, design: .monospaced))
                            .foregroundColor(Color(hex: "#4ADE80"))
                        Text("−\(summary.linesRemoved)")
                            .font(.system(size: 28, weight: .semibold, design: .monospaced))
                            .foregroundColor(Color(hex: "#F87171"))
                    }
                    .padding(.top, 24)
                }
                Spacer()
                Text("Coucou · github.com/Louis-CFM/coucou")
                    .font(.system(size: 20, weight: .medium, design: .monospaced))
                    .foregroundColor(Color(hex: "#8E939C").opacity(0.6)).padding(.bottom, 60)
            }
        }
        .frame(width: 1080, height: 1920)
    }
    @ViewBuilder private func statBlock(_ value: String, label: String) -> some View {
        VStack(spacing: 8) {
            Text(value).font(.system(size: 64, weight: .black, design: .rounded))
                .foregroundColor(Color(hex: "#F1F2F4")).minimumScaleFactor(0.5).lineLimit(1)
            Text(label).font(.system(size: 18, weight: .semibold))
                .foregroundColor(Color(hex: "#8E939C")).tracking(2)
        }.frame(maxWidth: .infinity)
    }
    private var weekLabel: String {
        let f = DateFormatter(); f.dateFormat = "MMM d"
        return "\(f.string(from: summary.weekStart)) – \(f.string(from: summary.weekEnd))"
    }
    private func fmtDur(_ m: Int) -> String {
        if m < 60 { return "\(m)m" }
        let h = m / 60; let r = m % 60
        return r == 0 ? "\(h)h" : "\(h)h \(r)m"
    }
}

// Build a PreviewSummary from the fake data.
let previewTotalSecs = fakeTurns.reduce(0.0) { $0 + $1.end.timeIntervalSince($1.start) }
let previewSummary = PreviewSummary(
    weekStart:             lastMonday,
    weekEnd:               cal.date(byAdding: .second, value: -1, to: weekEnd)!,
    totalMinutes:          Int(previewTotalSecs / 60),
    sessionCount:          fakeTurns.count,
    filesChanged:          fakeTurns.reduce(0) { $0 + $1.filesChanged },
    linesAdded:            fakeTurns.reduce(0) { $0 + $1.linesAdded },
    linesRemoved:          fakeTurns.reduce(0) { $0 + $1.linesRemoved },
    commandsRun:           fakeTurns.reduce(0) { $0 + $1.commandsRun },
    questionsAnswered:     fakeTurns.reduce(0) { $0 + $1.questions },
    permissionsAllowed:    fakeDecisions.filter { $0.decision == "allow" || $0.decision == "always" }.count,
    permissionsDenied:     fakeDecisions.filter { $0.decision == "deny" }.count,
    topAgent:              "Claude Code",
    topProject:            "coucou",
    busiestDay:            "Friday",
    longestSessionMinutes: Int(fakeTurns.map { $0.end.timeIntervalSince($0.start) }.max()! / 60)
)

@MainActor
func renderShareCard() {
    let view = PreviewShareImageView(summary: previewSummary)
    let renderer = ImageRenderer(content: view)
    renderer.proposedSize = ProposedViewSize(width: 1080, height: 1920)
    renderer.scale = 1
    guard let cgImage = renderer.cgImage else { print("ImageRenderer: cgImage nil"); return }
    let nsImage = NSImage(cgImage: cgImage, size: NSSize(width: 1080, height: 1920))
    guard let tiff = nsImage.tiffRepresentation,
          let rep  = NSBitmapImageRep(data: tiff),
          let png  = rep.representation(using: .png, properties: [:]) else {
        print("ImageRenderer: PNG encode failed"); return
    }
    let desktop = FileManager.default.urls(for: .desktopDirectory, in: .userDomainMask)[0]
    let pngURL  = desktop.appendingPathComponent("coucou-recap-preview.png")
    do {
        try png.write(to: pngURL, options: .atomic)
        print("Share card PNG → \(pngURL.path)")
    } catch {
        print("ImageRenderer: write failed: \(error)")
    }
}

// Render on the main actor via a Task, then exit.
Task { @MainActor in
    renderShareCard()
    exit(0)
}
RunLoop.main.run()
