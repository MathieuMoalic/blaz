import 'package:flutter/material.dart';
import 'package:intl/intl.dart';
import '../../api.dart';
import '../../widgets/recipe_card.dart';

/// Shows a bottom sheet listing the next [dayCount] days with their already-
/// assigned recipes so the user can pick a day to assign [recipeTitle] to.
/// Returns the chosen ISO date string, or null if cancelled.
Future<String?> showDayPickerSheet({
  required BuildContext context,
  required String recipeTitle,
  int dayCount = 14,
}) {
  final today = DateTime.now();
  final fmt = DateFormat('yyyy-MM-dd');
  final days = List.generate(dayCount, (i) => today.add(Duration(days: i)));

  return showModalBottomSheet<String>(
    context: context,
    isScrollControlled: true,
    showDragHandle: true,
    builder: (ctx) => _DayPickerSheet(
      recipeTitle: recipeTitle,
      days: days,
      fmt: fmt,
    ),
  );
}

class _DayPickerSheet extends StatefulWidget {
  final String recipeTitle;
  final List<DateTime> days;
  final DateFormat fmt;

  const _DayPickerSheet({
    required this.recipeTitle,
    required this.days,
    required this.fmt,
  });

  @override
  State<_DayPickerSheet> createState() => _DayPickerSheetState();
}

class _DayPickerSheetState extends State<_DayPickerSheet> {
  // iso -> meal plan entries (null = not yet loaded)
  final Map<String, List<MealPlanEntry>> _cache = {};

  @override
  void initState() {
    super.initState();
    _loadAll();
  }

  Future<void> _loadAll() async {
    final results = await Future.wait(
      widget.days.map((d) => fetchMealPlanForDay(widget.fmt.format(d))),
    );
    if (!mounted) return;
    setState(() {
      for (int i = 0; i < widget.days.length; i++) {
        _cache[widget.fmt.format(widget.days[i])] = results[i];
      }
    });
  }

  String _label(DateTime d) {
    final today = DateTime.now();
    final diff = d.difference(DateTime(today.year, today.month, today.day)).inDays;
    if (diff == 0) return 'Today';
    if (diff == 1) return 'Tomorrow';
    return DateFormat('EEE, MMM d').format(d);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final media = MediaQuery.of(context);
    return SafeArea(
      child: SizedBox(
        height: media.size.height * 0.75,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 4, 16, 12),
              child: Text(
                'Assign "${widget.recipeTitle}" to…',
                style: theme.textTheme.titleMedium,
              ),
            ),
            const Divider(height: 1),
            Expanded(
              child: ListView.separated(
                padding: const EdgeInsets.symmetric(vertical: 6),
                itemCount: widget.days.length,
                separatorBuilder: (_, __) => const Divider(height: 1),
                itemBuilder: (_, i) {
                  final d = widget.days[i];
                  final iso = widget.fmt.format(d);
                  final entries = _cache[iso];

                  return ListTile(
                    onTap: () => Navigator.pop(context, iso),
                    title: Text(
                      _label(d),
                      style: theme.textTheme.bodyLarge?.copyWith(
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    subtitle: entries == null
                        ? const LinearProgressIndicator(minHeight: 2)
                        : entries.isEmpty
                        ? Text(
                            'Nothing planned',
                            style: theme.textTheme.bodySmall?.copyWith(
                              color: theme.colorScheme.outline,
                            ),
                          )
                        : _MiniCardRow(entries: entries),
                    trailing: const Icon(Icons.chevron_right),
                  );
                },
              ),
            ),
            const Divider(height: 1),
            Padding(
              padding: EdgeInsets.fromLTRB(
                16,
                10,
                16,
                media.viewInsets.bottom + 12,
              ),
              child: OutlinedButton(
                onPressed: () => Navigator.pop(context),
                child: const Text('Cancel'),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Horizontally scrollable row of mini recipe cards for already-assigned recipes.
class _MiniCardRow extends StatelessWidget {
  final List<MealPlanEntry> entries;
  const _MiniCardRow({required this.entries});

  @override
  Widget build(BuildContext context) {
    const cardWidth = 80.0;
    return Padding(
      padding: const EdgeInsets.only(top: 6, bottom: 4),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: entries.map((e) {
            final imgUrl = mediaUrl('recipes/${e.recipeId}/small.webp');
            return Padding(
              padding: const EdgeInsets.only(right: 8),
              child: RecipeCard(
                title: e.title,
                imageUrl: imgUrl,
                width: cardWidth,
              ),
            );
          }).toList(),
        ),
      ),
    );
  }
}
