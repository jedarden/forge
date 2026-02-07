#!/usr/bin/env python3
"""
Control Panel Cost Management System
Implements use-or-lose and pay-per-token optimization strategies
"""

from datetime import datetime, timedelta
from typing import Dict, List, Optional, Tuple
from dataclasses import dataclass
from enum import Enum
import json


class ServiceType(Enum):
    SUBSCRIPTION = "subscription"
    API = "api"


class ModelTier(Enum):
    ULTRA_BUDGET = "ultra_budget"
    BUDGET = "budget"
    BALANCED = "balanced"
    PREMIUM = "premium"


@dataclass
class Model:
    """Model configuration and pricing"""
    name: str
    provider: str
    service_type: ServiceType
    input_cost_per_mtok: float
    output_cost_per_mtok: float
    context_window: int
    quality_score: int  # 0-100

    @property
    def blended_cost_per_mtok(self) -> float:
        """Assuming 1:3 input:output ratio"""
        return (self.input_cost_per_mtok + 3 * self.output_cost_per_mtok) / 4


@dataclass
class Subscription:
    """Subscription service with quota tracking"""
    name: str
    model: Model
    monthly_cost: float
    billing_period_start: datetime
    billing_period_end: datetime
    estimated_quota_tokens: int
    used_tokens: int = 0

    @property
    def remaining_tokens(self) -> int:
        return max(0, self.estimated_quota_tokens - self.used_tokens)

    @property
    def utilization_pct(self) -> float:
        return (self.used_tokens / self.estimated_quota_tokens) * 100

    @property
    def days_left(self) -> int:
        delta = self.billing_period_end - datetime.now()
        return max(0, delta.days)

    @property
    def billing_period_days(self) -> int:
        delta = self.billing_period_end - self.billing_period_start
        return delta.days

    @property
    def time_remaining_pct(self) -> float:
        if self.billing_period_days == 0:
            return 0.0
        return (self.days_left / self.billing_period_days) * 100

    @property
    def quota_remaining_pct(self) -> float:
        return (self.remaining_tokens / self.estimated_quota_tokens) * 100

    def has_quota_remaining(self) -> bool:
        return self.remaining_tokens > 0

    def use_tokens(self, tokens: int) -> bool:
        """Attempt to use tokens from quota. Returns True if successful."""
        if tokens <= self.remaining_tokens:
            self.used_tokens += tokens
            return True
        return False

    @property
    def cost_savings_vs_api(self) -> float:
        """Calculate savings vs paying API rates"""
        api_cost = (self.used_tokens / 1_000_000) * self.model.blended_cost_per_mtok
        return api_cost - self.monthly_cost


@dataclass
class Task:
    """Task to be executed"""
    id: str
    description: str
    estimated_tokens: int

    # Value scoring factors
    affects_revenue: bool = False
    reduces_costs: bool = False
    improves_efficiency: bool = False
    deadline_hours: Optional[float] = None
    requires_perfect_accuracy: bool = False
    requires_high_accuracy: bool = False
    customer_facing: bool = False
    executive_review: bool = False
    team_review: bool = False
    minimum_quality_required: int = 60  # 0-100

    # Task metadata
    created_at: datetime = None
    priority_override: Optional[int] = None  # Manual override 0-100

    def __post_init__(self):
        if self.created_at is None:
            self.created_at = datetime.now()
        if self.deadline_hours is None:
            self.deadline_hours = 168  # Default 1 week


class TaskValueScorer:
    """
    Scores task value to determine appropriate model investment
    """

    WEIGHTS = {
        'business_impact': 0.30,
        'time_sensitivity': 0.25,
        'complexity': 0.20,
        'quality_requirement': 0.15,
        'visibility': 0.10,
    }

    def score_task(self, task: Task) -> int:
        """Calculate task value score (0-100)"""

        # Allow manual override
        if task.priority_override is not None:
            return task.priority_override

        score = 0.0

        # Business Impact (0-100)
        if task.affects_revenue:
            impact_score = 100
        elif task.reduces_costs:
            impact_score = 80
        elif task.improves_efficiency:
            impact_score = 50
        else:
            impact_score = 20
        score += impact_score * self.WEIGHTS['business_impact']

        # Time Sensitivity (0-100)
        if task.deadline_hours < 4:
            time_score = 100
        elif task.deadline_hours < 24:
            time_score = 75
        elif task.deadline_hours < 168:  # 1 week
            time_score = 50
        else:
            time_score = 25
        score += time_score * self.WEIGHTS['time_sensitivity']

        # Complexity (0-100)
        complexity_score = min(100, (task.estimated_tokens / 50_000) * 100)
        score += complexity_score * self.WEIGHTS['complexity']

        # Quality Requirement (0-100)
        if task.requires_perfect_accuracy:
            quality_score = 100
        elif task.requires_high_accuracy:
            quality_score = 70
        else:
            quality_score = 40
        score += quality_score * self.WEIGHTS['quality_requirement']

        # Visibility (0-100)
        if task.customer_facing:
            visibility_score = 100
        elif task.executive_review:
            visibility_score = 80
        elif task.team_review:
            visibility_score = 50
        else:
            visibility_score = 20
        score += visibility_score * self.WEIGHTS['visibility']

        return int(score)


class QuotaOptimizer:
    """
    Implements use-or-lose optimization for subscriptions
    """

    def calculate_urgency(self, subscription: Subscription) -> float:
        """
        Calculate how aggressively we should use a subscription
        Returns: urgency_score (0.0 to 1.0)
        """
        quota_remaining_pct = subscription.quota_remaining_pct / 100
        time_remaining_pct = subscription.time_remaining_pct / 100

        # If we have more time than quota remaining, we're under-utilizing
        if time_remaining_pct < quota_remaining_pct:
            urgency = (quota_remaining_pct - time_remaining_pct) / max(0.01, quota_remaining_pct)
        else:
            urgency = 0.0

        # Exponential urgency in final 3 days
        if subscription.days_left <= 3:
            days_urgency = 0.8 + (0.2 * (3 - subscription.days_left) / 3)
            urgency = max(urgency, days_urgency)

        return min(1.0, urgency)

    def should_accelerate(self, subscription: Subscription) -> Tuple[bool, str]:
        """
        Determine if we should accelerate usage of a subscription
        """
        urgency = self.calculate_urgency(subscription)

        if urgency > 0.8:
            return True, f"EMERGENCY: {subscription.quota_remaining_pct:.1f}% quota remains with {subscription.days_left} days left"
        elif urgency > 0.5:
            return True, f"ACCELERATE: {subscription.quota_remaining_pct:.1f}% quota remains with {subscription.days_left} days left"
        elif urgency > 0.3:
            return True, f"MONITOR: {subscription.quota_remaining_pct:.1f}% quota remains with {subscription.days_left} days left"
        else:
            return False, f"ON TRACK: {subscription.quota_remaining_pct:.1f}% quota utilization"

    def generate_acceleration_tasks(self, subscription: Subscription) -> List[str]:
        """
        Generate suggestions for using remaining quota productively
        """
        suggestions = []

        remaining = subscription.remaining_tokens

        if remaining > 10_000_000:  # 10M+ tokens
            suggestions.extend([
                "Generate comprehensive documentation for entire codebase",
                "Create extensive test suites with edge cases",
                "Perform security audit on all components",
                "Generate architecture diagrams and design docs",
                "Create onboarding materials and tutorials",
                "Analyze performance optimization opportunities across system",
            ])
        elif remaining > 5_000_000:  # 5M+ tokens
            suggestions.extend([
                "Generate documentation for major modules",
                "Create test coverage for critical paths",
                "Perform code review on recent changes",
                "Generate API documentation",
                "Create deployment guides",
            ])
        elif remaining > 1_000_000:  # 1M+ tokens
            suggestions.extend([
                "Generate inline code comments",
                "Create unit tests for new features",
                "Perform refactoring analysis",
                "Generate README files",
            ])
        else:  # Under 1M tokens
            suggestions.extend([
                "Generate commit messages",
                "Quick code reviews",
                "Simple documentation updates",
            ])

        return suggestions


class ModelSelector:
    """
    Selects optimal model based on task value and cost-benefit analysis
    """

    # Model tier definitions
    MODEL_TIERS = {
        ModelTier.ULTRA_BUDGET: {
            'quality': 60,
            'avg_cost_per_mtok': 0.50,
            'value_range': (0, 30),
        },
        ModelTier.BUDGET: {
            'quality': 70,
            'avg_cost_per_mtok': 2.00,
            'value_range': (30, 50),
        },
        ModelTier.BALANCED: {
            'quality': 85,
            'avg_cost_per_mtok': 10.00,
            'value_range': (50, 75),
        },
        ModelTier.PREMIUM: {
            'quality': 95,
            'avg_cost_per_mtok': 40.00,
            'value_range': (75, 100),
        },
    }

    def __init__(self, available_models: Dict[str, Model]):
        self.available_models = available_models
        self.value_scorer = TaskValueScorer()

    def select_tier_by_value(self, task_value: int) -> ModelTier:
        """Select appropriate tier based on task value"""
        for tier, config in self.MODEL_TIERS.items():
            min_val, max_val = config['value_range']
            if min_val <= task_value < max_val:
                return tier
        return ModelTier.PREMIUM  # Default to premium for 100+ value

    def select_model_from_tier(self, tier: ModelTier, task: Task) -> Optional[Model]:
        """
        Select cheapest available model in tier that meets requirements
        """
        tier_config = self.MODEL_TIERS[tier]
        min_quality = tier_config['quality']

        # Filter models by quality requirement and API type
        candidates = [
            model for model in self.available_models.values()
            if model.service_type == ServiceType.API
            and model.quality_score >= min_quality
            and model.context_window >= task.estimated_tokens
        ]

        if not candidates:
            return None

        # Return cheapest model in tier
        return min(candidates, key=lambda m: m.blended_cost_per_mtok)

    def select_optimal_model(self, task: Task) -> Tuple[Model, int]:
        """
        Select most cost-effective model meeting task requirements
        Returns: (model, task_value_score)
        """
        task_value = self.value_scorer.score_task(task)
        tier = self.select_tier_by_value(task_value)
        model = self.select_model_from_tier(tier, task)

        if model is None:
            # Fallback to cheapest overall if tier unavailable
            api_models = [m for m in self.available_models.values()
                         if m.service_type == ServiceType.API]
            model = min(api_models, key=lambda m: m.blended_cost_per_mtok)

        return model, task_value


class CostBenefitAnalyzer:
    """
    Performs cost-benefit analysis for task execution decisions
    """

    # Mapping task value scores to dollar values
    DOLLAR_VALUE_MAP = {
        (0, 30): 5,
        (30, 50): 25,
        (50, 75): 100,
        (75, 100): 500,
    }

    MIN_ROI = 2.0  # Require at least 2x return

    def estimate_task_value_dollars(self, task_value_score: int) -> float:
        """Convert task value score to estimated dollar value"""
        for (min_score, max_score), value in self.DOLLAR_VALUE_MAP.items():
            if min_score <= task_value_score < max_score:
                return value
        return 1000  # Very high value tasks

    def estimate_success_probability(self, model: Model, task: Task) -> float:
        """Estimate probability of successful task completion"""
        quality_gap = model.quality_score - task.minimum_quality_required

        if quality_gap >= 20:
            return 0.95
        elif quality_gap >= 10:
            return 0.85
        elif quality_gap >= 0:
            return 0.70
        else:
            return 0.50

    def calculate_expected_value(self, task: Task, model: Model,
                                task_value_score: int) -> Dict:
        """
        Calculate expected value of using a specific model for a task
        EV = (Task Value × Success Probability) - Cost
        """
        task_dollar_value = self.estimate_task_value_dollars(task_value_score)
        success_probability = self.estimate_success_probability(model, task)

        # Calculate model cost
        estimated_tokens = task.estimated_tokens
        model_cost = (estimated_tokens / 1_000_000) * model.blended_cost_per_mtok

        # Calculate expected value
        expected_benefit = task_dollar_value * success_probability
        expected_value = expected_benefit - model_cost
        roi = (expected_benefit / model_cost) if model_cost > 0 else float('inf')

        return {
            'expected_value': expected_value,
            'expected_benefit': expected_benefit,
            'cost': model_cost,
            'success_probability': success_probability,
            'roi': roi,
            'task_dollar_value': task_dollar_value,
        }

    def should_execute_task(self, task: Task, model: Model,
                          task_value_score: int,
                          budget_remaining: float) -> Tuple[bool, str, Dict]:
        """
        Decide whether to execute a task given budget constraints
        """
        ev_analysis = self.calculate_expected_value(task, model, task_value_score)

        # Reject if expected value is negative
        if ev_analysis['expected_value'] < 0:
            return False, "Negative expected value", ev_analysis

        # Reject if cost exceeds remaining budget
        if ev_analysis['cost'] > budget_remaining:
            return False, "Insufficient budget", ev_analysis

        # Require minimum ROI threshold
        if ev_analysis['roi'] < self.MIN_ROI:
            return False, f"ROI {ev_analysis['roi']:.1f}x below threshold {self.MIN_ROI}x", ev_analysis

        return True, f"Approved - ROI {ev_analysis['roi']:.1f}x", ev_analysis


class BudgetManager:
    """
    Manages daily/weekly/monthly API spending budgets
    """

    def __init__(self, monthly_budget: float):
        self.monthly_budget = monthly_budget
        self.daily_budget = monthly_budget / 30
        self.weekly_budget = monthly_budget / 4

        self.spent_today = 0.0
        self.spent_this_week = 0.0
        self.spent_this_month = 0.0

    def can_afford(self, cost: float) -> bool:
        """Check if cost fits within remaining budget"""
        daily_remaining = self.daily_budget - self.spent_today
        weekly_remaining = self.weekly_budget - self.spent_this_week
        monthly_remaining = self.monthly_budget - self.spent_this_month

        return (cost <= daily_remaining and
                cost <= weekly_remaining and
                cost <= monthly_remaining)

    def reserve_budget(self, cost: float, priority: int) -> bool:
        """
        Reserve budget for a task, potentially borrowing from future
        """
        if self.can_afford(cost):
            return True

        # High priority tasks can borrow from tomorrow's budget
        if priority >= 80:
            tomorrow_budget = self.daily_budget * 0.5
            if cost <= (self.daily_budget - self.spent_today + tomorrow_budget):
                return True

        return False

    def spend(self, cost: float):
        """Record spending"""
        self.spent_today += cost
        self.spent_this_week += cost
        self.spent_this_month += cost

    @property
    def monthly_remaining(self) -> float:
        return self.monthly_budget - self.spent_this_month

    @property
    def daily_remaining(self) -> float:
        return self.daily_budget - self.spent_today

    def get_status(self) -> Dict:
        """Get budget status summary"""
        return {
            'monthly': {
                'budget': self.monthly_budget,
                'spent': self.spent_this_month,
                'remaining': self.monthly_remaining,
                'pct_used': (self.spent_this_month / self.monthly_budget) * 100,
            },
            'daily': {
                'budget': self.daily_budget,
                'spent': self.spent_today,
                'remaining': self.daily_remaining,
                'pct_used': (self.spent_today / self.daily_budget) * 100,
            },
            'weekly': {
                'budget': self.weekly_budget,
                'spent': self.spent_this_week,
                'remaining': self.weekly_budget - self.spent_this_week,
                'pct_used': (self.spent_this_week / self.weekly_budget) * 100,
            }
        }


class ControlPanel:
    """
    Main orchestrator for cost optimization
    Combines use-or-lose and pay-per-token strategies
    """

    def __init__(self,
                 subscriptions: List[Subscription],
                 available_models: Dict[str, Model],
                 monthly_budget: float):
        self.subscriptions = subscriptions
        self.available_models = available_models
        self.budget_manager = BudgetManager(monthly_budget)

        self.quota_optimizer = QuotaOptimizer()
        self.model_selector = ModelSelector(available_models)
        self.cost_analyzer = CostBenefitAnalyzer()
        self.value_scorer = TaskValueScorer()

    def allocate_task(self, task: Task) -> Tuple[Optional[Model], Optional[Subscription], Dict]:
        """
        Allocate a task to the optimal service (subscription or API)
        Returns: (model, subscription_if_used, decision_metadata)
        """
        metadata = {
            'task_id': task.id,
            'task_value': self.value_scorer.score_task(task),
            'timestamp': datetime.now().isoformat(),
        }

        # Check subscriptions first
        for subscription in self.subscriptions:
            if not subscription.has_quota_remaining():
                continue

            urgency = self.quota_optimizer.calculate_urgency(subscription)

            # High urgency or high-value tasks go to subscription
            if urgency > 0.5 or metadata['task_value'] >= 50:
                if subscription.use_tokens(task.estimated_tokens):
                    metadata['service'] = 'subscription'
                    metadata['service_name'] = subscription.name
                    metadata['cost'] = 0  # Already paid
                    metadata['urgency'] = urgency
                    metadata['savings'] = (task.estimated_tokens / 1_000_000) * subscription.model.blended_cost_per_mtok
                    return subscription.model, subscription, metadata

        # Fall back to API selection
        model, task_value = self.model_selector.select_optimal_model(task)
        metadata['task_value'] = task_value

        # Perform cost-benefit analysis
        should_execute, reason, ev_analysis = self.cost_analyzer.should_execute_task(
            task, model, task_value, self.budget_manager.monthly_remaining
        )

        metadata['service'] = 'api'
        metadata['model'] = model.name
        metadata['cost'] = ev_analysis['cost']
        metadata['expected_value'] = ev_analysis['expected_value']
        metadata['roi'] = ev_analysis['roi']
        metadata['decision'] = reason

        if should_execute:
            self.budget_manager.spend(ev_analysis['cost'])
            return model, None, metadata
        else:
            return None, None, metadata

    def get_optimization_report(self) -> Dict:
        """Generate comprehensive optimization report"""
        report = {
            'timestamp': datetime.now().isoformat(),
            'budget': self.budget_manager.get_status(),
            'subscriptions': [],
            'recommendations': [],
        }

        # Subscription status
        for sub in self.subscriptions:
            urgency = self.quota_optimizer.calculate_urgency(sub)
            should_accel, message = self.quota_optimizer.should_accelerate(sub)

            sub_report = {
                'name': sub.name,
                'utilization_pct': sub.utilization_pct,
                'remaining_tokens': sub.remaining_tokens,
                'days_left': sub.days_left,
                'urgency_score': urgency,
                'status': message,
                'cost_savings': sub.cost_savings_vs_api,
            }

            if should_accel:
                sub_report['acceleration_suggestions'] = self.quota_optimizer.generate_acceleration_tasks(sub)

            report['subscriptions'].append(sub_report)

        # Generate recommendations
        total_savings = sum(sub.cost_savings_vs_api for sub in self.subscriptions)
        report['recommendations'].append(f"Total savings from subscriptions: ${total_savings:.2f}")

        budget_status = self.budget_manager.get_status()
        if budget_status['monthly']['pct_used'] > 80:
            report['recommendations'].append("WARNING: Over 80% of monthly budget used")

        return report

    def simulate_monthly_costs(self, expected_tokens: int,
                              task_distribution: Dict[str, float]) -> Dict:
        """
        Simulate expected monthly costs given token volume and task distribution

        task_distribution example:
        {
            'low_value': 0.60,    # 60% low value tasks
            'medium_value': 0.25, # 25% medium value
            'high_value': 0.15,   # 15% high value
        }
        """
        simulation = {
            'expected_tokens': expected_tokens,
            'task_distribution': task_distribution,
            'subscription_usage': {},
            'api_usage': {},
            'total_cost': 0.0,
            'cost_per_mtok': 0.0,
        }

        # Calculate subscription coverage
        total_subscription_quota = sum(sub.estimated_quota_tokens for sub in self.subscriptions)
        subscription_cost = sum(sub.monthly_cost for sub in self.subscriptions)

        tokens_from_subscriptions = min(expected_tokens, total_subscription_quota)
        overflow_tokens = max(0, expected_tokens - total_subscription_quota)

        simulation['subscription_usage'] = {
            'quota_tokens': total_subscription_quota,
            'used_tokens': tokens_from_subscriptions,
            'cost': subscription_cost,
        }

        # Calculate API overflow costs by task tier
        api_cost = 0.0
        for task_type, pct in task_distribution.items():
            tokens = overflow_tokens * pct

            if task_type == 'low_value':
                # Use DeepSeek
                cost = (tokens / 1_000_000) * 0.245
            elif task_type == 'medium_value':
                # Use Haiku
                cost = (tokens / 1_000_000) * 1.0
            else:  # high_value
                # Use Sonnet
                cost = (tokens / 1_000_000) * 12.0

            api_cost += cost
            simulation['api_usage'][task_type] = {
                'tokens': tokens,
                'cost': cost,
            }

        simulation['total_cost'] = subscription_cost + api_cost
        simulation['cost_per_mtok'] = (simulation['total_cost'] / expected_tokens) * 1_000_000

        return simulation


def demo():
    """Demonstration of the cost optimization system"""

    # Define models
    models = {
        'deepseek': Model('DeepSeek V3', 'deepseek', ServiceType.API, 0.14, 0.28, 64000, 65),
        'haiku': Model('Claude 3 Haiku', 'anthropic', ServiceType.API, 0.25, 1.25, 200000, 70),
        'sonnet': Model('Claude 3.5 Sonnet', 'anthropic', ServiceType.API, 3.0, 15.0, 200000, 90),
        'opus': Model('Claude 3 Opus', 'anthropic', ServiceType.API, 15.0, 75.0, 200000, 98),
        'gpt4o': Model('GPT-4o', 'openai', ServiceType.API, 2.5, 10.0, 128000, 88),
    }

    # Define subscriptions
    billing_start = datetime.now() - timedelta(days=20)
    billing_end = billing_start + timedelta(days=30)

    subscriptions = [
        Subscription(
            name='Claude Pro',
            model=models['sonnet'],
            monthly_cost=20.0,
            billing_period_start=billing_start,
            billing_period_end=billing_end,
            estimated_quota_tokens=10_000_000,
            used_tokens=3_200_000,
        ),
        Subscription(
            name='Cursor Pro',
            model=models['sonnet'],
            monthly_cost=20.0,
            billing_period_start=billing_start,
            billing_period_end=billing_end,
            estimated_quota_tokens=5_000_000,
            used_tokens=3_100_000,
        ),
    ]

    # Initialize optimizer
    optimizer = ControlPanel(
        subscriptions=subscriptions,
        available_models=models,
        monthly_budget=100.0,
    )

    # Create sample tasks
    tasks = [
        Task(
            id='T1',
            description='Generate comprehensive test suite',
            estimated_tokens=50_000,
            affects_revenue=False,
            improves_efficiency=True,
            deadline_hours=48,
            requires_high_accuracy=True,
        ),
        Task(
            id='T2',
            description='Fix critical production bug',
            estimated_tokens=30_000,
            affects_revenue=True,
            deadline_hours=2,
            requires_perfect_accuracy=True,
            customer_facing=True,
        ),
        Task(
            id='T3',
            description='Generate documentation',
            estimated_tokens=100_000,
            improves_efficiency=True,
            deadline_hours=168,
        ),
    ]

    # Allocate tasks
    print("=" * 80)
    print("POOL OPTIMIZER - COST OPTIMIZATION DEMO")
    print("=" * 80)
    print()

    for task in tasks:
        print(f"Task: {task.id} - {task.description}")
        print(f"Estimated tokens: {task.estimated_tokens:,}")

        model, subscription, metadata = optimizer.allocate_task(task)

        if model:
            print(f"Allocated to: {metadata['service']}")
            if subscription:
                print(f"  Subscription: {metadata['service_name']}")
                print(f"  Cost: $0 (subscription)")
                print(f"  Savings: ${metadata['savings']:.2f}")
                print(f"  Urgency: {metadata['urgency']:.2f}")
            else:
                print(f"  Model: {metadata['model']}")
                print(f"  Cost: ${metadata['cost']:.2f}")
                print(f"  Expected Value: ${metadata['expected_value']:.2f}")
                print(f"  ROI: {metadata['roi']:.1f}x")
            print(f"  Task Value: {metadata['task_value']}/100")
        else:
            print(f"REJECTED: {metadata['decision']}")

        print()

    # Generate report
    print("=" * 80)
    print("OPTIMIZATION REPORT")
    print("=" * 80)
    print()

    report = optimizer.get_optimization_report()
    print(json.dumps(report, indent=2, default=str))

    # Simulate monthly costs
    print()
    print("=" * 80)
    print("MONTHLY COST SIMULATION")
    print("=" * 80)
    print()

    simulation = optimizer.simulate_monthly_costs(
        expected_tokens=25_000_000,
        task_distribution={
            'low_value': 0.60,
            'medium_value': 0.25,
            'high_value': 0.15,
        }
    )

    print(json.dumps(simulation, indent=2, default=str))


if __name__ == '__main__':
    demo()
