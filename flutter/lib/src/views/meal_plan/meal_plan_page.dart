import 'dart:async';
import 'package:flutter/material.dart';
import 'package:intl/intl.dart';
import 'package:scrollable_positioned_list/scrollable_positioned_list.dart';
import '../../api.dart';
import '../recipe_detail_page.dart';
import 'day_timeline_box.dart';
import 'recipe_picker_sheet.dart';

class MealPlanPage extends StatefulWidget {
  const MealPlanPage({super.key});
  @override
  State<MealPlanPage> createState() => MealPlanPageState();
}

// RENAMED to public
class MealPlanPageState extends State<MealPlanPage> {
  static const int _pastDays = 60;
  static const int _futureDays = 30;
  static const int _initialWarmCount = 8;

  final _fmt = DateFormat('yyyy-MM-dd');
  late final DateTime _today;

  late final List<int> _dayOffsets;

  final Map<String, Future<List<MealPlanEntry>>> _futures = {};
  final Map<String, List<MealPlanEntry>> _cache = {};
  final Map<int, Recipe> _recipeIndex = {};

  List<PrepReminderDto> _reminders = [];
  bool _remindersLoaded = false;

  final _itemScrollController = ItemScrollController();
  final _itemPositionsListener = ItemPositionsListener.create();

  bool _refreshing = false; // NEW

  @override
  void initState() {
    super.initState();
    _today = _stripTime(DateTime.now());

    _dayOffsets = List.generate(
      _pastDays + _futureDays + 1,
      (i) => i - _pastDays,
    );

    final firstWarmIndex = _pastDays;
    final lastWarmIndex = (_pastDays + _initialWarmCount).clamp(
      0,
      _dayOffsets.length - 1,
    );

    for (int i = firstWarmIndex; i <= lastWarmIndex; i++) {
      final iso = _dayForOffset(_dayOffsets[i]);
      _futures.putIfAbsent(iso, () => fetchMealPlanForDay(iso));
    }

    _warmRecipeIndex();
    _loadReminders();

    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_itemScrollController.isAttached) {
        _itemScrollController.jumpTo(index: _pastDays, alignment: 0.0);
      }
    });
  }

  DateTime _stripTime(DateTime d) => DateTime(d.year, d.month, d.day);

  String _dayForOffset(int offset) =>
      _fmt.format(_today.add(Duration(days: offset)));

  String _labelForOffset(int offset) {
    if (offset == 0) return 'Today';
    if (offset == 1) return 'Tomorrow';
    if (offset == -1) return 'Yesterday';
    final date = _today.add(Duration(days: offset));
    return DateFormat('EEE, MMM d').format(date);
  }

  Color _railColorForOffset(int offset, ThemeData theme) {
    if (offset == 0) return theme.colorScheme.primary;
    if (offset < 0) return theme.colorScheme.secondary;
    return theme.colorScheme.tertiary;
  }

  Future<void> _warmRecipeIndex() async {
    try {
      final all = await fetchRecipes();
      if (!mounted) return;
      setState(() {
        for (final r in all) {
          _recipeIndex[r.id] = r;
        }
      });
    } catch (_) {}
  }

  Future<void> _loadReminders() async {
    try {
      final reminders = await fetchUpcomingReminders();
      if (!mounted) return;
      setState(() {
        _reminders = reminders;
        _remindersLoaded = true;
      });
    } catch (_) {
      if (mounted) setState(() => _remindersLoaded = true);
    }
  }

  List<String> _visibleDayIsos() {
    final positions = _itemPositionsListener.itemPositions.value;
    if (positions.isEmpty) {
      return [_dayForOffset(0)];
    }

    final indices = positions.map((p) => p.index).toList();
    indices.sort();
    final minI = (indices.first - 1).clamp(0, _dayOffsets.length - 1);
    final maxI = (indices.last + 1).clamp(0, _dayOffsets.length - 1);

    final days = <String>[];
    for (int i = minI; i <= maxI; i++) {
      days.add(_dayForOffset(_dayOffsets[i]));
    }
    return days;
  }

  /// Public refresh for HomeShell + pull-to-refresh.
  /// Re-fetches only visible (and near-visible) days to keep it light.
  Future<void> refresh() async {
    if (_refreshing) return;
    setState(() => _refreshing = true);

    final days = _visibleDayIsos();

    try {
      // Start day fetches without clearing UI.
      setState(() {
        for (final d in days) {
          _futures[d] = fetchMealPlanForDay(d);
        }
      });

      // In parallel, refresh recipe thumbnails/titles.
      final recipeFuture = _warmRecipeIndex();
      final reminderFuture = _loadReminders();

      final results = await Future.wait(days.map((d) => _futures[d]!));
      await recipeFuture;
      await reminderFuture;

      if (!mounted) return;
      setState(() {
        for (int i = 0; i < days.length; i++) {
          _cache[days[i]] = results[i];
        }
      });
    } finally {
      if (mounted) setState(() => _refreshing = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    // Reminders due today or tomorrow
    final today = _fmt.format(_today);
    final tomorrow = _fmt.format(_today.add(const Duration(days: 1)));
    final urgentReminders = _reminders
        .where((r) => r.dueDate == today || r.dueDate == tomorrow)
        .toList();

    return Column(
      children: [
        if (urgentReminders.isNotEmpty) _PrepReminderCard(reminders: urgentReminders),
        Expanded(
          child: Stack(
            children: [
              RefreshIndicator(
                onRefresh: refresh,
                child: ScrollablePositionedList.builder(
                  itemScrollController: _itemScrollController,
                  itemPositionsListener: _itemPositionsListener,
                  padding: const EdgeInsets.symmetric(vertical: 8),
                  itemCount: _dayOffsets.length,
                  itemBuilder: (context, i) {
                    final offset = _dayOffsets[i];
                    final dayIso = _dayForOffset(offset);
                    final isToday = offset == 0;

                    _futures.putIfAbsent(
                      dayIso,
                      () => fetchMealPlanForDay(dayIso),
                    );

                    return KeyedSubtree(
                      key: ValueKey(dayIso),
                      child: DayTimelineBox(
                        dayLabel: _labelForOffset(offset),
                        isToday: isToday,
                        dayIso: dayIso,
                        railColor: _railColorForOffset(offset, theme),
                        future: _futures[dayIso]!,
                        cached: _cache[dayIso],
                        recipeIndex: _recipeIndex,
                        onAssign: () => _openRecipePicker(dayIso),
                        onOpenRecipe: (id) async {
                          await Navigator.of(context).push(
                            MaterialPageRoute(
                              builder: (_) => RecipeDetailPage(recipeId: id),
                            ),
                          );
                        },
                        onUnassign: (meal) async {
                          await unassignRecipeFromDay(
                            day: meal.day,
                            recipeId: meal.recipeId,
                          );
                          // Keep targeted reload if you like:
                          await _reloadDay(dayIso);
                        },
                        onLoaded: (items) => _cache[dayIso] = items,
                      ),
                    );
                  },
                ),
              ),

              if (_refreshing)
                const Positioned(
                  left: 0,
                  right: 0,
                  top: 0,
                  child: LinearProgressIndicator(minHeight: 2),
                ),
            ],
          ),
        ),
      ],
    );
  }

  Future<void> _reloadDay(String dayIso) async {
    setState(() {
      _futures[dayIso] = fetchMealPlanForDay(dayIso);
    });
    final items = await _futures[dayIso]!;
    if (!mounted) return;
    setState(() => _cache[dayIso] = items);
  }

  Future<void> _openRecipePicker(String dayIso) async {
    final all = _recipeIndex.isNotEmpty
        ? _recipeIndex.values.toList()
        : await fetchRecipes();

    if (_recipeIndex.isEmpty) {
      setState(() {
        for (final r in all) {
          _recipeIndex[r.id] = r;
        }
      });
    }
    if (!mounted) return;

    final picked = await showRecipePickerSheet(
      context: context,
      dayIso: dayIso,
      all: all,
    );
    if (picked == null || picked.isEmpty) return;

    int ok = 0;
    final failures = <int>[];
    for (final id in picked) {
      try {
        await assignRecipeToDay(day: dayIso, recipeId: id);
        ok++;
      } catch (_) {
        failures.add(id);
      }
    }
    await _reloadDay(dayIso);

    if (!mounted) return;
    final msg = failures.isEmpty
        ? 'Assigned $ok recipe(s) to $dayIso'
        : 'Assigned $ok, failed ${failures.length} (IDs: ${failures.join(', ')})';
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }
}

class _PrepReminderCard extends StatelessWidget {
  final List<PrepReminderDto> reminders;
  const _PrepReminderCard({required this.reminders});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final today = DateFormat('yyyy-MM-dd').format(DateTime.now());

    return Card(
      margin: const EdgeInsets.fromLTRB(12, 8, 12, 0),
      color: theme.colorScheme.secondaryContainer,
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                const Icon(Icons.alarm, size: 16),
                const SizedBox(width: 6),
                Text('Prep reminders', style: theme.textTheme.labelLarge),
              ],
            ),
            const SizedBox(height: 8),
            for (final r in reminders) ...[
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    r.dueDate == today ? 'Today' : 'Tomorrow',
                    style: theme.textTheme.bodySmall?.copyWith(
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                  const SizedBox(width: 6),
                  Expanded(
                    child: Text(
                      '${r.step} — for ${r.recipeTitle} (${DateFormat('EEE d').format(DateTime.parse(r.mealDate))})',
                      style: theme.textTheme.bodySmall,
                    ),
                  ),
                ],
              ),
              if (r != reminders.last) const SizedBox(height: 4),
            ],
          ],
        ),
      ),
    );
  }
}
