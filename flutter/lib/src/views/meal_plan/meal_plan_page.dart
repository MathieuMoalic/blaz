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
  State<MealPlanPage> createState() => _MealPlanPageState();
}

class _MealPlanPageState extends State<MealPlanPage> {
  // how many days to show around today
  static const int _pastDays = 60; // shown above today (offscreen initially)
  static const int _futureDays = 30; // shown below today

  // warm enough visible days so content shows immediately
  static const int _initialWarmCount = 8;

  final _fmt = DateFormat('yyyy-MM-dd');
  late final DateTime _today;

  // Offsets from today: -_pastDays..0.._futureDays
  late final List<int> _dayOffsets;

  // Per-day async/cache
  final Map<String, Future<List<MealPlanEntry>>> _futures = {};
  final Map<String, List<MealPlanEntry>> _cache = {};
  final Map<int, Recipe> _recipeIndex = {}; // id -> Recipe (thumb/title)

  // Index-based list controllers
  final _itemScrollController = ItemScrollController();
  final _itemPositionsListener = ItemPositionsListener.create();

  @override
  void initState() {
    super.initState();
    _today = _stripTime(DateTime.now());

    // Build fixed range
    _dayOffsets = List.generate(
      _pastDays + _futureDays + 1,
      (i) => i - _pastDays,
    ); // [-past .. +future]

    // Warm today's future + a few below so we see data immediately
    final firstWarmIndex = _pastDays; // index of "today"
    final lastWarmIndex = (_pastDays + _initialWarmCount).clamp(
      0,
      _dayOffsets.length - 1,
    );
    for (int i = firstWarmIndex; i <= lastWarmIndex; i++) {
      final iso = _dayForOffset(_dayOffsets[i]);
      _futures.putIfAbsent(iso, () => fetchMealPlanForDay(iso));
    }

    // Warm recipe index (for thumbs/titles)
    _warmRecipeIndex();

    // After first frame, jump so TODAY is at the top of the viewport.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_itemScrollController.isAttached) {
        _itemScrollController.jumpTo(index: _pastDays, alignment: 0.0);
      }
    });
  }

  // Helpers
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
    } catch (_) {
      /* soft-fail */
    }
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

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Column(
      children: [
        Expanded(
          child: ScrollablePositionedList.builder(
            itemScrollController: _itemScrollController,
            itemPositionsListener: _itemPositionsListener,
            padding: const EdgeInsets.symmetric(vertical: 8),
            itemCount: _dayOffsets.length,
            itemBuilder: (context, i) {
              final offset = _dayOffsets[i];
              final dayIso = _dayForOffset(offset);
              final isToday = offset == 0;

              // Lazily create the future only when this row builds the first time.
              _futures.putIfAbsent(dayIso, () => fetchMealPlanForDay(dayIso));

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
                    await _reloadDay(dayIso);
                  },
                  onLoaded: (items) => _cache[dayIso] = items,
                ),
              );
            },
          ),
        ),
      ],
    );
  }
}
