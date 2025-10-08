import 'dart:async';
import 'package:flutter/material.dart';
import 'package:intl/intl.dart';
import '../api.dart';

class MealPlanPage extends StatefulWidget {
  const MealPlanPage({super.key});
  @override
  State<MealPlanPage> createState() => _MealPlanPageState();
}

class _MealPlanPageState extends State<MealPlanPage> {
  static const int _daysBefore = 30; // how many past days to show
  static const int _daysAfter = 30; // how many future days to show

  final _fmt = DateFormat('yyyy-MM-dd');
  final Map<String, Future<List<MealPlanEntry>>> _futures = {};
  final Map<String, List<MealPlanEntry>> _cache = {};

  late final DateTime _today;

  @override
  void initState() {
    super.initState();
    _today = _stripTime(DateTime.now());
    final d = _fmt.format(_today);
    _futures[d] = fetchMealPlanForDay(d);
  }

  DateTime _stripTime(DateTime d) => DateTime(d.year, d.month, d.day);

  String _dayForIndex(int i) {
    // index 0 is the oldest (today - _daysBefore)
    final start = _today.subtract(const Duration(days: _daysBefore));
    final date = start.add(Duration(days: i));
    return _fmt.format(date);
  }

  String _labelFor(DateTime date) {
    final diff = date.difference(_today).inDays;
    if (diff == 0) return 'Today';
    if (diff == 1) return 'Tomorrow';
    if (diff == -1) return 'Yesterday';
    return DateFormat('EEE, MMM d').format(date);
  }

  Color _dotColor(DateTime date, ThemeData theme) {
    final diff = date.difference(_today).inDays;
    if (diff == 0) return theme.colorScheme.primary;
    if (diff < 0) return theme.colorScheme.secondary;
    return theme.colorScheme.tertiary;
  }

  Future<void> _refreshAll() async {
    setState(() {
      _futures.clear();
      _cache.clear();
    });
  }

  Future<void> _reloadDay(String day) async {
    setState(() {
      _futures[day] = fetchMealPlanForDay(day);
    });
    final items = await _futures[day]!;
    if (!mounted) return;
    setState(() {
      _cache[day] = items;
    });
  }

  // --- Assign via search (by name/ingredients) ---

  bool _matchesRecipe(Recipe r, String q) {
    if (q.isEmpty) return true;
    final needle = q.toLowerCase();
    if (r.title.toLowerCase().contains(needle)) return true;

    // defensively check ingredients as List<String> or comma-separated String
    try {
      final dyn = r as dynamic;
      final v = dyn.ingredients;
      if (v == null) return false;
      if (v is List) {
        for (final e in v) {
          if (e.toString().toLowerCase().contains(needle)) return true;
        }
      } else if (v is String) {
        for (final part in v.split(',')) {
          if (part.trim().toLowerCase().contains(needle)) return true;
        }
      }
    } catch (_) {}
    return false;
  }

  Future<void> _openRecipePicker(String day) async {
    // load once
    final all = await fetchRecipes();

    final ctrl = TextEditingController();
    String query = '';
    final selected = <int>{};

    final picked = await showModalBottomSheet<Set<int>>(
      context: context,
      isScrollControlled: true,
      showDragHandle: true,
      builder: (ctx) {
        final media = MediaQuery.of(ctx);
        final height = media.size.height * 0.8;

        return StatefulBuilder(
          builder: (ctx, setSheetState) {
            final filtered = all.where((r) => _matchesRecipe(r, query)).toList()
              ..sort(
                (a, b) =>
                    a.title.toLowerCase().compareTo(b.title.toLowerCase()),
              );

            void toggle(int id) {
              setSheetState(() {
                if (!selected.add(id)) selected.remove(id);
              });
            }

            void selectAll(bool v) {
              setSheetState(() {
                selected.clear();
                if (v) for (final r in filtered) selected.add(r.id);
              });
            }

            final allSelected =
                filtered.isNotEmpty &&
                filtered.every((r) => selected.contains(r.id));
            final anySelected = filtered.any((r) => selected.contains(r.id));
            final triValue = allSelected
                ? true
                : anySelected
                ? null
                : false;

            return SafeArea(
              child: SizedBox(
                height: height,
                child: Column(
                  children: [
                    // Header
                    Padding(
                      padding: const EdgeInsets.fromLTRB(16, 10, 16, 6),
                      child: Row(
                        children: [
                          Expanded(
                            child: Text(
                              'Assign to $day',
                              style: Theme.of(ctx).textTheme.titleMedium,
                            ),
                          ),
                          const SizedBox(width: 8),
                          const Text('Select all'),
                          Checkbox(
                            value: triValue,
                            tristate: true,
                            onChanged: (_) => selectAll(!allSelected),
                          ),
                        ],
                      ),
                    ),
                    // Search field
                    Padding(
                      padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
                      child: TextField(
                        controller: ctrl,
                        textInputAction: TextInputAction.search,
                        decoration: InputDecoration(
                          hintText: 'Search recipes by name or ingredient',
                          prefixIcon: const Icon(Icons.search),
                          isDense: true,
                          border: OutlineInputBorder(
                            borderRadius: BorderRadius.circular(12),
                          ),
                          suffixIcon: query.isEmpty
                              ? null
                              : IconButton(
                                  tooltip: 'Clear',
                                  icon: const Icon(Icons.close),
                                  onPressed: () => setSheetState(() {
                                    ctrl.clear();
                                    query = '';
                                  }),
                                ),
                        ),
                        onChanged: (s) => setSheetState(() => query = s.trim()),
                      ),
                    ),
                    const Divider(height: 1),
                    // List
                    Expanded(
                      child: filtered.isEmpty
                          ? const Center(child: Text('No matching recipes'))
                          : ListView.separated(
                              padding: const EdgeInsets.symmetric(vertical: 6),
                              itemCount: filtered.length,
                              separatorBuilder: (_, __) =>
                                  const Divider(height: 1),
                              itemBuilder: (_, i) {
                                final r = filtered[i];
                                final thumb = mediaUrl(
                                  r.imagePathSmall ?? r.imagePathFull,
                                );
                                final checked = selected.contains(r.id);
                                return ListTile(
                                  onTap: () => toggle(r.id),
                                  leading: thumb == null
                                      ? const SizedBox(
                                          width: 44,
                                          height: 44,
                                          child: Icon(
                                            Icons.image_not_supported,
                                          ),
                                        )
                                      : ClipRRect(
                                          borderRadius: BorderRadius.circular(
                                            6,
                                          ),
                                          child: Image.network(
                                            thumb,
                                            width: 44,
                                            height: 44,
                                            fit: BoxFit.cover,
                                          ),
                                        ),
                                  title: Text(r.title),
                                  subtitle: (r.ingredients.isEmpty)
                                      ? null
                                      : Text(
                                          r.ingredients.take(3).join(', '),
                                          maxLines: 1,
                                          overflow: TextOverflow.ellipsis,
                                        ),
                                  trailing: Checkbox(
                                    value: checked,
                                    onChanged: (_) => toggle(r.id),
                                  ),
                                );
                              },
                            ),
                    ),
                    const Divider(height: 1),
                    // Actions
                    Padding(
                      padding: EdgeInsets.only(
                        left: 16,
                        right: 16,
                        top: 10,
                        bottom: MediaQuery.of(ctx).viewInsets.bottom + 12,
                      ),
                      child: Row(
                        children: [
                          Expanded(
                            child: OutlinedButton(
                              onPressed: () => Navigator.pop(ctx),
                              child: const Text('Cancel'),
                            ),
                          ),
                          const SizedBox(width: 12),
                          Expanded(
                            child: FilledButton.icon(
                              onPressed: selected.isEmpty
                                  ? null
                                  : () => Navigator.pop(
                                      ctx,
                                      Set<int>.from(selected),
                                    ),
                              icon: const Icon(Icons.event_available),
                              label: Text(
                                selected.isEmpty
                                    ? 'Assign'
                                    : 'Assign ${selected.length}',
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
            );
          },
        );
      },
    );

    if (picked == null || picked.isEmpty) return;

    // Assign sequentially (clear errors shown at end)
    int ok = 0;
    final failures = <int>[];
    for (final id in picked) {
      try {
        await assignRecipeToDay(day: day, recipeId: id);
        ok++;
      } catch (_) {
        failures.add(id);
      }
    }
    await _reloadDay(day);

    if (!mounted) return;
    final msg = failures.isEmpty
        ? 'Assigned $ok recipe(s) to $day'
        : 'Assigned $ok, failed ${failures.length} (IDs: ${failures.join(', ')})';
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  @override
  Widget build(BuildContext context) {
    final total = _daysBefore + 1 + _daysAfter; // center on today
    final theme = Theme.of(context);

    return Column(
      children: [
        // Header (no calendar/today button)
        Container(
          height: 56,
          padding: const EdgeInsets.symmetric(horizontal: 12),
          alignment: Alignment.centerLeft,
          child: Text('Meal plan', style: theme.textTheme.titleLarge),
        ),

        // Timeline list
        Expanded(
          child: RefreshIndicator(
            onRefresh: _refreshAll,
            child: ListView.builder(
              padding: const EdgeInsets.symmetric(vertical: 8),
              itemCount: total,
              itemBuilder: (context, i) {
                final dayStr = _dayForIndex(i);
                final date = _fmt.parse(dayStr);
                _futures.putIfAbsent(dayStr, () => fetchMealPlanForDay(dayStr));

                return _DayTimelineBox(
                  dayLabel: _labelFor(date),
                  dayIso: dayStr,
                  dotColor: _dotColor(date, theme),
                  future: _futures[dayStr]!,
                  cached: _cache[dayStr],
                  onAssign: () => _openRecipePicker(dayStr),
                  onUnassign: (meal) async {
                    await unassignRecipeFromDay(
                      day: meal.day,
                      recipeId: meal.recipeId,
                    );
                    await _reloadDay(dayStr);
                  },
                  onLoaded: (items) => _cache[dayStr] = items,
                );
              },
            ),
          ),
        ),
      ],
    );
  }
}

class _DayTimelineBox extends StatelessWidget {
  final String dayLabel; // e.g., Today / Tue, Oct 7
  final String dayIso; // yyyy-MM-dd
  final Color dotColor;
  final Future<List<MealPlanEntry>> future;
  final List<MealPlanEntry>? cached;
  final VoidCallback onAssign;
  final Future<void> Function(MealPlanEntry) onUnassign;
  final void Function(List<MealPlanEntry>) onLoaded;

  const _DayTimelineBox({
    super.key,
    required this.dayLabel,
    required this.dayIso,
    required this.dotColor,
    required this.future,
    required this.cached,
    required this.onAssign,
    required this.onUnassign,
    required this.onLoaded,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Timeline rail
          SizedBox(width: 28, child: _Rail(dotColor: dotColor)),
          // Day card
          Expanded(
            child: Card(
              elevation: 1.5,
              shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(14),
              ),
              child: Padding(
                padding: const EdgeInsets.fromLTRB(12, 10, 12, 8),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    // Day header row
                    Row(
                      children: [
                        Text(dayLabel, style: theme.textTheme.titleMedium),
                        const Spacer(),
                        IconButton(
                          tooltip: 'Assign recipes',
                          icon: const Icon(Icons.add_circle_outline),
                          onPressed: onAssign,
                        ),
                      ],
                    ),
                    const SizedBox(height: 4),
                    // Entries
                    FutureBuilder<List<MealPlanEntry>>(
                      future: future,
                      builder: (context, snap) {
                        if (snap.connectionState != ConnectionState.done) {
                          return const Padding(
                            padding: EdgeInsets.symmetric(vertical: 12),
                            child: SizedBox(
                              height: 24,
                              child: Center(
                                child: CircularProgressIndicator(
                                  strokeWidth: 2,
                                ),
                              ),
                            ),
                          );
                        }
                        if (snap.hasError) {
                          return Padding(
                            padding: const EdgeInsets.only(bottom: 8),
                            child: Text('Error: ${snap.error}'),
                          );
                        }

                        final items = snap.data ?? const <MealPlanEntry>[];
                        onLoaded(items);

                        if (items.isEmpty) {
                          return Padding(
                            padding: const EdgeInsets.only(bottom: 8),
                            child: Text(
                              'No recipes',
                              style: theme.textTheme.bodyMedium?.copyWith(
                                color: theme.colorScheme.onSurfaceVariant,
                              ),
                            ),
                          );
                        }

                        return Column(
                          children: [
                            for (final m in items)
                              _MealRow(entry: m, onDelete: () => onUnassign(m)),
                          ],
                        );
                      },
                    ),
                  ],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _Rail extends StatelessWidget {
  final Color dotColor;
  const _Rail({required this.dotColor});

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, c) {
        final h = c.maxHeight;
        return Stack(
          alignment: Alignment.topCenter,
          children: [
            Positioned.fill(
              top: 0,
              bottom: 0,
              child: Center(
                child: Container(
                  width: 2,
                  height: h,
                  color: Theme.of(context).dividerColor,
                ),
              ),
            ),
            Container(
              width: 12,
              height: 12,
              decoration: BoxDecoration(
                color: dotColor,
                shape: BoxShape.circle,
                boxShadow: const [
                  BoxShadow(blurRadius: 4, offset: Offset(0, 1)),
                ],
              ),
            ),
          ],
        );
      },
    );
  }
}

class _MealRow extends StatelessWidget {
  final MealPlanEntry entry;
  final VoidCallback onDelete;
  const _MealRow({required this.entry, required this.onDelete});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(entry.title, style: theme.textTheme.bodyLarge),
                Text(
                  'Recipe #${entry.recipeId}',
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
          IconButton(
            tooltip: 'Unassign',
            icon: const Icon(Icons.delete_outline),
            onPressed: onDelete,
          ),
        ],
      ),
    );
  }
}
