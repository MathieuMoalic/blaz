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
  static const int _daysBefore = 30;
  static const int _daysAfter = 30;

  final _fmt = DateFormat('yyyy-MM-dd');
  final Map<String, Future<List<MealPlanEntry>>> _futures = {};
  final Map<String, List<MealPlanEntry>> _cache = {};
  final Map<int, Recipe> _recipeIndex = {}; // id -> Recipe (thumb/title)

  late final DateTime _today;

  @override
  void initState() {
    super.initState();
    _today = _stripTime(DateTime.now());
    final d = _fmt.format(_today);
    _futures[d] = fetchMealPlanForDay(d);
    _warmRecipeIndex();
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

  DateTime _stripTime(DateTime d) => DateTime(d.year, d.month, d.day);

  String _dayForIndex(int i) {
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

  Color _railColor(DateTime date, ThemeData theme) {
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
    await _warmRecipeIndex();
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

  // ---------- Assign via search (name/ingredients) ----------
  bool _matchesRecipe(Recipe r, String q) {
    if (q.isEmpty) return true;
    final needle = q.toLowerCase();
    if (r.title.toLowerCase().contains(needle)) return true;
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
                            onChanged: (_) {
                              setSheetState(() {
                                selected.clear();
                                if (!allSelected) {
                                  for (final r in filtered) selected.add(r.id);
                                }
                              });
                            },
                          ),
                        ],
                      ),
                    ),
                    // Search
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
                    // Results
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
    final total = _daysBefore + 1 + _daysAfter;
    final theme = Theme.of(context);

    return Column(
      children: [
        // Header (simple)
        Container(
          height: 56,
          padding: const EdgeInsets.symmetric(horizontal: 12),
          alignment: Alignment.centerLeft,
          child: Text('Meal plan', style: theme.textTheme.titleLarge),
        ),

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
                  railColor: _railColor(date, theme),
                  future: _futures[dayStr]!,
                  cached: _cache[dayStr],
                  recipeIndex: _recipeIndex,
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
  final String dayLabel; // Today / Tue, Oct 7
  final String dayIso; // yyyy-MM-dd
  final Color railColor; // vertical rail
  final Future<List<MealPlanEntry>> future;
  final List<MealPlanEntry>? cached;
  final Map<int, Recipe> recipeIndex; // id -> Recipe
  final VoidCallback onAssign;
  final Future<void> Function(MealPlanEntry) onUnassign;
  final void Function(List<MealPlanEntry>) onLoaded;

  const _DayTimelineBox({
    super.key,
    required this.dayLabel,
    required this.dayIso,
    required this.railColor,
    required this.future,
    required this.cached,
    required this.recipeIndex,
    required this.onAssign,
    required this.onUnassign,
    required this.onLoaded,
  });
  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      child: IntrinsicHeight(
        // <-- lets the rail match the card height
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            // Vertical rail (no dot)
            VerticalDivider(
              width: 20, // total horizontal space it occupies
              thickness: 2, // line thickness
              indent: 0,
              endIndent: 0,
              color: railColor.withOpacity(0.6),
            ),

            // Day card
            Expanded(
              child: Card(
                elevation: 1.5,
                shape: RoundedRectangleBorder(
                  borderRadius: BorderRadius.circular(14),
                ),
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(12, 10, 12, 10),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      // Header
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
                      const SizedBox(height: 6),

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
                              padding: const EdgeInsets.only(bottom: 4),
                              child: Text(
                                'No recipes',
                                style: theme.textTheme.bodyMedium?.copyWith(
                                  color: theme.colorScheme.onSurfaceVariant,
                                ),
                              ),
                            );
                          }

                          // Horizontal strip of thumbnails (2 fit; >2 scrolls)
                          const tileWidth = 150.0;
                          const tileHeight = 180.0;
                          return SizedBox(
                            height: tileHeight,
                            child: ListView.separated(
                              scrollDirection: Axis.horizontal,
                              primary: false,
                              padding: const EdgeInsets.symmetric(
                                horizontal: 2,
                              ),
                              itemCount: items.length,
                              separatorBuilder: (_, __) =>
                                  const SizedBox(width: 10),
                              itemBuilder: (_, i) {
                                final m = items[i];
                                final recipe = recipeIndex[m.recipeId];
                                final title = recipe?.title ?? m.title;
                                final imageUrl = mediaUrl(
                                  recipe?.imagePathSmall ??
                                      recipe?.imagePathFull,
                                );
                                return _RecipeThumbTile(
                                  width: tileWidth,
                                  height: tileHeight,
                                  title: title,
                                  imageUrl: imageUrl,
                                  onDelete: () => onUnassign(m),
                                );
                              },
                            ),
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
      ),
    );
  }
}

class _RecipeThumbTile extends StatelessWidget {
  final double width;
  final double height;
  final String title;
  final String? imageUrl;
  final VoidCallback onDelete;

  const _RecipeThumbTile({
    required this.width,
    required this.height,
    required this.title,
    required this.imageUrl,
    required this.onDelete,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SizedBox(
      width: width,
      height: height,
      child: Card(
        elevation: 1,
        clipBehavior: Clip.antiAlias,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            // image with delete overlay
            AspectRatio(
              aspectRatio: 4 / 3,
              child: Stack(
                fit: StackFit.expand,
                children: [
                  imageUrl == null
                      ? const _ThumbPlaceholder()
                      : Image.network(
                          imageUrl!,
                          fit: BoxFit.cover,
                          errorBuilder: (_, __, ___) =>
                              const _ThumbPlaceholder(),
                        ),
                  Positioned(
                    top: 6,
                    right: 6,
                    child: Material(
                      color: Colors.black54,
                      shape: const CircleBorder(),
                      child: IconButton(
                        tooltip: 'Unassign',
                        icon: const Icon(
                          Icons.delete_outline,
                          size: 18,
                          color: Colors.white,
                        ),
                        splashRadius: 18,
                        onPressed: onDelete,
                      ),
                    ),
                  ),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(8, 8, 8, 8),
              child: Text(
                title,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                textAlign: TextAlign.center,
                style: theme.textTheme.bodyMedium,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _ThumbPlaceholder extends StatelessWidget {
  const _ThumbPlaceholder();
  @override
  Widget build(BuildContext context) {
    return Container(
      color: Theme.of(context).colorScheme.surfaceVariant.withOpacity(0.4),
      child: const Center(child: Icon(Icons.restaurant_menu)),
    );
  }
}
