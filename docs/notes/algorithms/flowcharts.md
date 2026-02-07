# Algorithm Flowcharts and Diagrams

## 1. Complete System Flow

```
┌───────────────────────────────────────────────────────────────────────────┐
│                         CONTROL PANEL SYSTEM                              │
└───────────────────────────────────────────────────────────────────────────┘

                              ┌─────────────┐
                              │ Task Intake │
                              │ (from Beads)│
                              └──────┬──────┘
                                     │
                                     ▼
                        ┌────────────────────────┐
                        │  Parse Task Metadata   │
                        │  - Priority            │
                        │  - Labels              │
                        │  - Description         │
                        │  - Dependencies        │
                        └──────────┬─────────────┘
                                   │
                   ┌───────────────┴────────────────┐
                   │                                │
                   ▼                                ▼
        ┌──────────────────┐            ┌──────────────────┐
        │ Estimate         │            │ Calculate        │
        │ Complexity       │            │ Value Score      │
        │                  │            │                  │
        │ • File count     │            │ • Priority: 40pts│
        │ • LOC estimate   │            │ • Complexity: 2x │
        │ • Dependencies   │            │ • Time bonus: +15│
        │ • Keywords       │            │ • Domain: 1.2x   │
        │                  │            │ • Risk: 1.3x     │
        └────────┬─────────┘            └─────────┬────────┘
                 │                                │
                 │                                │
                 └────────────┬───────────────────┘
                              │
                              ▼
                   ┌────────────────────────┐
                   │ Estimate Token Usage   │
                   │                        │
                   │ Input: 40-100K tokens  │
                   │ Output: 5-15K tokens   │
                   └──────────┬─────────────┘
                              │
                              ▼
                   ┌────────────────────────┐
                   │ Calculate Value Density│
                   │                        │
                   │ VD = Score / (Tok/1K)  │
                   │ VD = 85 / 55 = 1.55    │
                   └──────────┬─────────────┘
                              │
                              ▼
                   ┌────────────────────────┐
                   │ Check A/B Test         │
                   │ Assignment             │
                   └──────────┬─────────────┘
                              │
                   ┌──────────┴──────────┐
                   │                     │
           Control │                     │ Treatment
                   ▼                     ▼
        ┌────────────────┐    ┌────────────────────┐
        │ Use Standard   │    │ Use Experimental   │
        │ Algorithm      │    │ Algorithm          │
        └────────┬───────┘    └─────────┬──────────┘
                 │                      │
                 └──────────┬───────────┘
                            │
                            ▼
              ┌──────────────────────────┐
              │ Select Model Tier        │
              │                          │
              │ Score >= 90: Premium     │
              │ Score >= 75: Mid-Premium │
              │ Score >= 60: Mid-Range   │
              │ Score >= 40: Budget      │
              └──────────┬───────────────┘
                         │
                         ▼
              ┌──────────────────────────┐
              │ Check Subscription Quota │
              │                          │
              │ Claude Pro: 3.8M remain  │
              │ ChatGPT+: Available      │
              └──────────┬───────────────┘
                         │
              ┌──────────┴──────────────┐
              │                         │
       Quota  │                         │ No Quota
     Available│                         │
              ▼                         ▼
   ┌───────────────────┐    ┌───────────────────┐
   │ Use Subscription  │    │ Check API Budget  │
   │ Model             │    │                   │
   │ (Sonnet 4.5)      │    │ Remaining: $45.20 │
   └─────────┬─────────┘    └─────────┬─────────┘
             │                        │
             │              ┌─────────┴──────────┐
             │              │                    │
             │       Budget │                    │ No Budget
             │       OK     │                    │
             │              ▼                    ▼
             │    ┌──────────────┐    ┌──────────────┐
             │    │ Use API Model│    │ Downgrade or │
             │    │ (same tier)  │    │ Defer Task   │
             │    └──────┬───────┘    └──────────────┘
             │           │
             └───────────┴─────────────┐
                                       │
                                       ▼
                            ┌────────────────────┐
                            │ Assign Model       │
                            │                    │
                            │ Task → Sonnet 4.5  │
                            └──────────┬─────────┘
                                       │
                                       ▼
                            ┌────────────────────┐
                            │ Update Quota       │
                            │ Tracking           │
                            │                    │
                            │ Claude: -55K tokens│
                            └──────────┬─────────┘
                                       │
                                       ▼
                            ┌────────────────────┐
                            │ Assign to Worker   │
                            │                    │
                            │ Worker Pool: Sonnet│
                            └──────────┬─────────┘
                                       │
                                       ▼
                            ┌────────────────────┐
                            │ Execute Task       │
                            │                    │
                            │ Start: 14:32:10    │
                            └──────────┬─────────┘
                                       │
                                       ▼
                            ┌────────────────────┐
                            │ Track Execution    │
                            │                    │
                            │ • Actual tokens    │
                            │ • Completion time  │
                            │ • Success/failure  │
                            └──────────┬─────────┘
                                       │
                                       ▼
                            ┌────────────────────┐
                            │ Calculate Quality  │
                            │                    │
                            │ • Tests: 15/15     │
                            │ • Bugs: 0          │
                            │ • Score: 9.2/10    │
                            └──────────┬─────────┘
                                       │
                                       ▼
                            ┌────────────────────┐
                            │ Record Outcome     │
                            │                    │
                            │ • Database         │
                            │ • Performance cache│
                            └──────────┬─────────┘
                                       │
                                       ▼
                            ┌────────────────────┐
                            │ Update Learning    │
                            │ Metrics            │
                            │                    │
                            │ • Model perf +1    │
                            │ • Token accuracy   │
                            │ • Value correlation│
                            └────────────────────┘
```

## 2. Value Scoring Calculation Flow

```
┌──────────────────────────────────────────────────────────────┐
│                    VALUE SCORING FLOW                         │
└──────────────────────────────────────────────────────────────┘

Task Input: "Fix authentication vulnerability in production API"
Priority: P0
Labels: [security, backend, hotfix]
Files: 3
LOC: ~150

                        ┌─────────────┐
                        │   START     │
                        └──────┬──────┘
                               │
                ┌──────────────┴───────────────┐
                │  Get Priority Weight         │
                │  P0 → 40 points              │
                └──────────────┬───────────────┘
                               │
                ┌──────────────┴───────────────┐
                │  Estimate Complexity         │
                │                              │
                │  Files: 3 (moderate)         │
                │  LOC: 150 (moderate)         │
                │  Keywords: none (not complex)│
                │                              │
                │  → Complexity: MODERATE      │
                │  → Multiplier: 1.0x          │
                └──────────────┬───────────────┘
                               │
                ┌──────────────┴───────────────┐
                │  Calculate Base Score        │
                │                              │
                │  40 × 1.0 = 40               │
                └──────────────┬───────────────┘
                               │
                ┌──────────────┴───────────────┐
                │  Check Time Sensitivity      │
                │                              │
                │  Label: 'hotfix'             │
                │  → Immediate (+15)           │
                └──────────────┬───────────────┘
                               │
                ┌──────────────┴───────────────┐
                │  Add Time Bonus              │
                │                              │
                │  40 + 15 = 55                │
                └──────────────┬───────────────┘
                               │
                ┌──────────────┴───────────────┐
                │  Classify Domain             │
                │                              │
                │  Label: 'backend'            │
                │  → Domain: Backend           │
                │  → Modifier: 1.1x            │
                └──────────────┬───────────────┘
                               │
                ┌──────────────┴───────────────┐
                │  Apply Domain Modifier       │
                │                              │
                │  55 × 1.1 = 60.5             │
                └──────────────┬───────────────┘
                               │
                ┌──────────────┴───────────────┐
                │  Assess Risk Level           │
                │                              │
                │  Label: 'security'           │
                │  Description: 'production'   │
                │  → Risk: Production          │
                │  → Modifier: 1.3x            │
                └──────────────┬───────────────┘
                               │
                ┌──────────────┴───────────────┐
                │  Apply Risk Modifier         │
                │                              │
                │  60.5 × 1.3 = 78.65          │
                └──────────────┬───────────────┘
                               │
                ┌──────────────┴───────────────┐
                │  Round Final Score           │
                │                              │
                │  78.65 → 79                  │
                └──────────────┬───────────────┘
                               │
                        ┌──────┴──────┐
                        │   RESULT    │
                        │             │
                        │ Score: 79/100│
                        │ Tier: High   │
                        └─────────────┘
```

## 3. Model Selection Decision Tree

```
                              ┌─────────────┐
                              │ Task Value  │
                              │ Score: 79   │
                              └──────┬──────┘
                                     │
                          ┌──────────┴──────────┐
                          │ Score >= 90?        │
                          └──────────┬──────────┘
                                     │ NO
                          ┌──────────┴──────────┐
                          │ Score >= 75?        │
                          └──────────┬──────────┘
                                     │ YES
                          ┌──────────┴──────────┐
                          │ Target Tier:        │
                          │ MID-PREMIUM         │
                          │                     │
                          │ Candidates:         │
                          │ • Sonnet 4.5        │
                          │ • GPT-4 Turbo       │
                          │ • Opus 4.6          │
                          └──────────┬──────────┘
                                     │
                          ┌──────────┴──────────┐
                          │ Estimate Tokens     │
                          │                     │
                          │ Input: 27.5K        │
                          │ Output: 3.5K        │
                          │ Total: 31K          │
                          └──────────┬──────────┘
                                     │
                          ┌──────────┴──────────┐
                          │ Check Claude Pro    │
                          │ Subscription        │
                          └──────────┬──────────┘
                                     │
                          ┌──────────┴──────────────┐
                          │ Quota Available?        │
                          │                         │
                          │ Remaining: 3,800,000    │
                          │ Needed: 31,000          │
                          └──────────┬──────────────┘
                                     │ YES
                          ┌──────────┴──────────┐
                          │ Select:             │
                          │ SONNET 4.5          │
                          │ (Subscription)      │
                          │                     │
                          │ Cost: $0 (included) │
                          │ Value: 79 points    │
                          │ ROI: ∞              │
                          └─────────────────────┘

Alternative Flow (No Quota):

                          ┌──────────┴──────────┐
                          │ Quota Available?    │
                          └──────────┬──────────┘
                                     │ NO
                          ┌──────────┴──────────┐
                          │ Check API Budget    │
                          │                     │
                          │ Remaining: $45.20   │
                          └──────────┬──────────┘
                                     │
                          ┌──────────┴──────────┐
                          │ Calculate Cost      │
                          │                     │
                          │ Sonnet API:         │
                          │ $0.08 + $0.05 = $0.13│
                          │                     │
                          │ GPT-4 Turbo:        │
                          │ $0.28 + $0.10 = $0.38│
                          └──────────┬──────────┘
                                     │
                          ┌──────────┴──────────┐
                          │ Budget OK?          │
                          │                     │
                          │ $0.13 < $45.20 ✓    │
                          └──────────┬──────────┘
                                     │ YES
                          ┌──────────┴──────────┐
                          │ Select:             │
                          │ SONNET 4.5 (API)    │
                          │                     │
                          │ Cost: $0.13         │
                          │ Value: 79 points    │
                          │ ROI: 607.7          │
                          └─────────────────────┘
```

## 4. Batch Assignment Optimization Flow

```
┌────────────────────────────────────────────────────────────┐
│              BATCH ASSIGNMENT OPTIMIZATION                  │
│         Strategy: Maximize Subscription Usage               │
└────────────────────────────────────────────────────────────┘

Input: 10 tasks to assign
Claude Pro Quota: 5,000,000 tokens/month remaining

Step 1: Score All Tasks
┌──────┬──────────┬───────┬──────────┬─────────┐
│ Task │ Priority │ Score │ Tokens   │ Density │
├──────┼──────────┼───────┼──────────┼─────────┤
│ T1   │ P0       │ 95    │ 150,000  │ 0.63    │
│ T2   │ P0       │ 88    │ 40,000   │ 2.20    │
│ T3   │ P1       │ 72    │ 30,000   │ 2.40    │
│ T4   │ P1       │ 68    │ 80,000   │ 0.85    │
│ T5   │ P2       │ 45    │ 15,000   │ 3.00    │
│ T6   │ P2       │ 42    │ 25,000   │ 1.68    │
│ T7   │ P3       │ 28    │ 10,000   │ 2.80    │
│ T8   │ P3       │ 25    │ 12,000   │ 2.08    │
│ T9   │ P4       │ 18    │ 8,000    │ 2.25    │
│ T10  │ P4       │ 12    │ 5,000    │ 2.40    │
└──────┴──────────┴───────┴──────────┴─────────┘

Step 2: Sort by Value (Descending)
┌──────┬───────┬──────────┐
│ T1   │ 95    │ 150,000  │ ← Assign Opus (Premium)
│ T2   │ 88    │ 40,000   │ ← Assign Sonnet (Subscription)
│ T3   │ 72    │ 30,000   │ ← Assign Sonnet (Subscription)
│ T4   │ 68    │ 80,000   │ ← Assign Sonnet (Subscription)
│ T5   │ 45    │ 15,000   │ ← Assign Sonnet (Subscription)
│ T6   │ 42    │ 25,000   │ ← Assign GLM-4.7 (Budget)
│ T7   │ 28    │ 10,000   │ ← Assign GLM-4.7 (Budget)
│ T8   │ 25    │ 12,000   │ ← Defer (low value)
│ T9   │ 18    │ 8,000    │ ← Defer (low value)
│ T10  │ 12    │ 5,000    │ ← Defer (low value)
└──────┴───────┴──────────┘

Step 3: Track Quota Usage
┌───────────────┬──────────────┬─────────────┐
│ Subscription  │ Used         │ Remaining   │
├───────────────┼──────────────┼─────────────┤
│ Claude Pro    │              │             │
│ Initial       │ 0            │ 5,000,000   │
│ After T2      │ 40,000       │ 4,960,000   │
│ After T3      │ 70,000       │ 4,930,000   │
│ After T4      │ 150,000      │ 4,850,000   │
│ After T5      │ 165,000      │ 4,835,000   │
│ Final         │ 165,000      │ 4,835,000   │
└───────────────┴──────────────┴─────────────┘

Step 4: Calculate API Costs
┌──────┬─────────┬──────────┬──────┐
│ Task │ Model   │ Tokens   │ Cost │
├──────┼─────────┼──────────┼──────┤
│ T1   │ Opus    │ 150,000  │ $2.25│
│ T6   │ GLM-4.7 │ 25,000   │ $0.00│
│ T7   │ GLM-4.7 │ 10,000   │ $0.00│
│      │         │          │      │
│ Total API Cost │          │ $2.25│
└──────┴─────────┴──────────┴──────┘

Step 5: Summary
┌────────────────────────────┬─────────┐
│ Total Value Delivered      │ 438 pts │
│ Subscription Tasks         │ 4       │
│ API Tasks                  │ 3       │
│ Deferred Tasks             │ 3       │
│ Total Cost                 │ $2.25   │
│ Subscription Utilization   │ 3.3%    │
│ Value per Dollar           │ 194.7   │
└────────────────────────────┴─────────┘
```

## 5. Learning Feedback Loop

```
┌────────────────────────────────────────────────────────────┐
│                   LEARNING FEEDBACK LOOP                    │
└────────────────────────────────────────────────────────────┘

                    ┌─────────────────┐
                    │ Task Executed   │
                    │                 │
                    │ Model: Sonnet   │
                    │ Task Type: P1_  │
                    │   backend_mod   │
                    └────────┬────────┘
                             │
                             ▼
              ┌──────────────────────────┐
              │ Collect Execution Data   │
              │                          │
              │ Predicted Tokens: 30,000 │
              │ Actual Tokens: 38,000    │
              │ Ratio: 1.27              │
              │                          │
              │ Predicted Score: 72      │
              │ Quality Score: 8.5       │
              │ Success: Yes             │
              └────────┬─────────────────┘
                       │
        ┌──────────────┴─────────────┐
        │                            │
        ▼                            ▼
┌────────────────┐         ┌────────────────────┐
│ Update Model   │         │ Update Token       │
│ Performance    │         │ Estimation         │
│                │         │                    │
│ Sonnet + P1_   │         │ P2_backend_mod:    │
│ backend_mod:   │         │                    │
│ • Avg Quality: │         │ • Input ratio: 1.2 │
│   8.7 → 8.6    │         │ • Suggest: +20%    │
│ • Success: 94% │         │   base estimate    │
└────────┬───────┘         └────────┬───────────┘
         │                          │
         └──────────┬───────────────┘
                    │
                    ▼
         ┌────────────────────┐
         │ Daily Batch Job    │
         │ (2:00 AM)          │
         │                    │
         │ Analyze last 30d:  │
         │ • 247 tasks        │
         │ • 15 adjustments   │
         │ • 3 A/B tests      │
         └─────────┬──────────┘
                   │
        ┌──────────┴─────────────┐
        │                        │
        ▼                        ▼
┌────────────────┐    ┌──────────────────┐
│ Calibrate      │    │ Analyze Cost     │
│ Parameters     │    │ Efficiency       │
│                │    │                  │
│ • Domain mod   │    │ • Downgrade T3   │
│   backend:     │    │   tasks from     │
│   1.1 → 1.15   │    │   Sonnet to      │
│                │    │   DeepSeek       │
│ • Confidence:  │    │                  │
│   0.82         │    │ • Save $1.2/task │
└────────┬───────┘    └──────────┬───────┘
         │                       │
         └──────────┬────────────┘
                    │
                    ▼
         ┌────────────────────┐
         │ A/B Test Check     │
         │                    │
         │ Test-002:          │
         │ • Control: n=52    │
         │ • Treatment: n=48  │
         │ • p-value: 0.03    │
         │ • Winner: Treatment│
         └─────────┬──────────┘
                   │
                   ▼
         ┌────────────────────┐
         │ Graduate to Prod   │
         │                    │
         │ Apply adjustment:  │
         │ backend_mod: 1.15  │
         │                    │
         │ Start new test:    │
         │ Test-004           │
         └────────────────────┘
```

## 6. Quota Management State Machine

```
┌────────────────────────────────────────────────────────────┐
│                 QUOTA MANAGEMENT STATES                     │
└────────────────────────────────────────────────────────────┘

                    ┌─────────────────┐
                    │  QUOTA_FULL     │
                    │                 │
                    │ > 80% remaining │
                    └────────┬────────┘
                             │
                  Use subscription for
                  high-value tasks (≥75)
                             │
                             ▼
               ┌──────────────────────────┐
               │ Token usage accumulates  │
               └──────────┬───────────────┘
                          │
                  ┌───────┴────────┐
                  │                │
            < 80% │                │ < 50%
                  ▼                ▼
        ┌─────────────────┐  ┌─────────────────┐
        │ QUOTA_MEDIUM    │  │ QUOTA_LOW       │
        │                 │  │                 │
        │ 50-80% remain   │  │ 20-50% remain   │
        └────────┬────────┘  └────────┬────────┘
                 │                    │
        Use for high          Use only for
        & medium value        critical tasks
        tasks (≥60)           (≥90)
                 │                    │
                 │          ┌─────────┴────────┐
                 │          │                  │
                 │    < 20% │                  │ Daily reset
                 │          ▼                  │
                 │  ┌─────────────────┐        │
                 │  │ QUOTA_CRITICAL  │        │
                 │  │                 │        │
                 │  │ < 20% remain    │        │
                 │  └────────┬────────┘        │
                 │           │                 │
                 │   Use API only,             │
                 │   save for emergencies      │
                 │           │                 │
                 │           │ < 5%            │
                 │           ▼                 │
                 │  ┌─────────────────┐        │
                 │  │ QUOTA_EXHAUSTED │        │
                 │  │                 │        │
                 │  │ 0% remain       │        │
                 │  └────────┬────────┘        │
                 │           │                 │
                 │   All tasks use API         │
                 │           │                 │
                 │           └─────────────────┼──┐
                 │                             │  │
                 └─────────────────────────────┘  │
                                                  │
                                         Midnight │
                                         (daily   │
                                          reset)  │
                                                  │
                                                  ▼
                                    ┌─────────────────┐
                                    │ QUOTA_FULL      │
                                    │                 │
                                    │ Reset to 100%   │
                                    └─────────────────┘

Alerts:
- QUOTA_LOW: Email notification
- QUOTA_CRITICAL: Slack alert
- QUOTA_EXHAUSTED: Page on-call
```

## 7. Quality Scoring Calculation

```
┌────────────────────────────────────────────────────────────┐
│              QUALITY SCORE CALCULATION (0-10)               │
└────────────────────────────────────────────────────────────┘

Task Completed: Add user authentication

              ┌─────────────────────┐
              │ Tests Passed: 15/15 │
              │ Component Score: 3.5│
              │ (max 4.0)           │
              └──────────┬──────────┘
                         │
              ┌──────────┴──────────┐
              │ Bugs Introduced: 0  │
              │ Component Score: 2.0│
              │ (max 2.0)           │
              └──────────┬──────────┘
                         │
              ┌──────────┴──────────┐
              │ Revision Needed: No │
              │ Component Score: 2.0│
              │ (max 2.0)           │
              └──────────┬──────────┘
                         │
              ┌──────────┴──────────┐
              │ Code Review: Good   │
              │ Component Score: 1.5│
              │ (max 2.0)           │
              └──────────┬──────────┘
                         │
              ┌──────────┴──────────┐
              │ Sum Components      │
              │                     │
              │ 3.5 + 2.0 + 2.0 + 1.5│
              │ = 9.0 / 10          │
              └──────────┬──────────┘
                         │
              ┌──────────┴──────────┐
              │ FINAL QUALITY       │
              │                     │
              │ 9.0 / 10            │
              │                     │
              │ Grade: Excellent    │
              └─────────────────────┘

Quality Grade Scale:
- 9.0-10.0: Excellent
- 7.5-8.9:  Good
- 6.0-7.4:  Acceptable
- 4.0-5.9:  Poor
- 0.0-3.9:  Failed
```

## 8. Token Estimation Decision Tree

```
                    ┌─────────────────┐
                    │ Task Complexity │
                    └────────┬────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
              ▼              ▼              ▼
       ┌──────────┐   ┌──────────┐   ┌──────────┐
       │ Simple   │   │ Moderate │   │ Complex  │
       │ 1-2 files│   │ 3-5 files│   │ 6+ files │
       └────┬─────┘   └────┬─────┘   └────┬─────┘
            │              │              │
            │              │              │
       ┌────┴─────┐   ┌────┴─────┐   ┌────┴─────┐
       │ Base:    │   │ Base:    │   │ Base:    │
       │ 10K/1.5K │   │ 27.5K/3.5K│  │ 70K/10K  │
       └────┬─────┘   └────┬─────┘   └────┬─────┘
            │              │              │
            └──────┬───────┴──────┬───────┘
                   │              │
                   ▼              ▼
            ┌─────────────────────────┐
            │ Domain Adjustment       │
            │                         │
            │ Infrastructure: +20%    │
            │ Documentation: +30% out │
            │ Testing: +40% output    │
            └──────────┬──────────────┘
                       │
                       ▼
            ┌─────────────────────────┐
            │ Workspace Adjustment    │
            │                         │
            │ Large (>100 files): +30%│
            │ Medium (>50 files): +15%│
            └──────────┬──────────────┘
                       │
                       ▼
            ┌─────────────────────────┐
            │ Apply Learned           │
            │ Adjustments             │
            │                         │
            │ backend_moderate: 1.15x │
            └──────────┬──────────────┘
                       │
                       ▼
            ┌─────────────────────────┐
            │ FINAL ESTIMATE          │
            │                         │
            │ Input: 31,625 tokens    │
            │ Output: 4,025 tokens    │
            │ Total: 35,650 tokens    │
            └─────────────────────────┘
```

These flowcharts provide visual representations of the key algorithms and processes in the control panel system.
