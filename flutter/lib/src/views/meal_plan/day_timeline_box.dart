import 'package:flutter/material.dart';
import '../../api.dart';
import 'recipe_thumb_tile.dart';

class DayTimelineBox extends StatefulWidget {
  final String dayLabel; // Today / Tue, Oct 7
  final String dayIso; // yyyy-MM-dd
  final bool isToday;
  final Color railColor; // kept for API compatibility (unused)
  final Future<List<MealPlanEntry>> future;
  final List<MealPlanEntry>? cached;
  final Map<int, Recipe> recipeIndex; // id -> Recipe
  final VoidCallback onAssign;
  final Future<void> Function(MealPlanEntry) onUnassign;
  final void Function(List<MealPlanEntry>) onLoaded;
  final void Function(int recipeId) onOpenRecipe;
  final Future<void> Function(MealPlanEntry meal)? onMoveMeal;

  const DayTimelineBox({
    super.key,
    required this.dayLabel,
    required this.dayIso,
    required this.isToday,
    required this.railColor,
    required this.future,
    required this.cached,
    required this.recipeIndex,
    required this.onAssign,
    required this.onUnassign,
    required this.onLoaded,
    required this.onOpenRecipe,
    this.onMoveMeal,
  });

  @override
  State<DayTimelineBox> createState() => _DayTimelineBoxState();
}

class _DayTimelineBoxState extends State<DayTimelineBox> {
  bool _isDragOver = false;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    final cardShape = RoundedRectangleBorder(
      side: _isDragOver
          ? BorderSide(color: theme.colorScheme.primary, width: 2)
          : widget.isToday
              ? BorderSide(color: theme.colorScheme.primary, width: 2)
              : BorderSide.none,
      borderRadius: BorderRadius.circular(14),
    );

    final cardColor = _isDragOver
        ? theme.colorScheme.primaryContainer.withValues(alpha: 0.25)
        : widget.isToday
            ? theme.colorScheme.primaryContainer.withValues(alpha: 0.08)
            : null;

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      child: DragTarget<MealPlanEntry>(
        onWillAcceptWithDetails: (details) =>
            details.data.day != widget.dayIso,
        onAcceptWithDetails: (details) {
          widget.onMoveMeal?.call(details.data);
        },
        onMove: (_) {
          if (!_isDragOver) setState(() => _isDragOver = true);
        },
        onLeave: (_) {
          if (_isDragOver) setState(() => _isDragOver = false);
        },
        builder: (context, candidateData, rejectedData) {
          return Card(
            elevation: widget.isToday ? 2.5 : 1.5,
            color: cardColor,
            shape: cardShape,
            clipBehavior: Clip.antiAlias,
            child: Padding(
              padding: const EdgeInsets.fromLTRB(12, 10, 12, 10),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  // Header
                  Row(
                    children: [
                      Text(
                        widget.dayLabel,
                        style: theme.textTheme.titleMedium?.copyWith(
                          fontWeight: widget.isToday ? FontWeight.w600 : null,
                        ),
                      ),
                      const Spacer(),
                      IconButton(
                        tooltip: 'Assign recipes',
                        icon: const Icon(Icons.add_circle_outline),
                        onPressed: widget.onAssign,
                      ),
                    ],
                  ),
                  const SizedBox(height: 6),

                  // Entries
                  FutureBuilder<List<MealPlanEntry>>(
                    future: widget.future,
                    builder: (context, snap) {
                      if (snap.connectionState != ConnectionState.done) {
                        return const Padding(
                          padding: EdgeInsets.symmetric(vertical: 12),
                          child: SizedBox(
                            height: 24,
                            child: Center(
                              child: CircularProgressIndicator(strokeWidth: 2),
                            ),
                          ),
                        );
                      }
                      if (snap.hasError) {
                        return Padding(
                          padding: const EdgeInsets.only(bottom: 8),
                          child: Row(
                            children: [
                              Icon(Icons.error_outline, size: 16, color: theme.colorScheme.error),
                              const SizedBox(width: 8),
                              Expanded(
                                child: Text(
                                  'Failed to load recipes for this day',
                                  style: TextStyle(fontSize: 12, color: theme.colorScheme.error),
                                ),
                              ),
                            ],
                          ),
                        );
                      }

                      final items = snap.data ?? const <MealPlanEntry>[];
                      widget.onLoaded(items);

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

                      // Make tiles ~10% smaller so two fit across with spacing.
                      const spacing = 10.0;
                      const horizontalPadding = 8.0;

                      return LayoutBuilder(
                        builder: (context, constraints) {
                          final maxW = constraints.maxWidth;
                          // Two tiles + one gap should fit; then shrink ~10%.
                          final base =
                              (maxW - horizontalPadding * 2 - spacing) / 2;
                          final tileWidth = base * 0.90;
                          // Preserve the old 150x180 aspect ratio (1.2).
                          final tileHeight = tileWidth * (180.0 / 150.0);

                          return SizedBox(
                            height: tileHeight,
                            child: ListView.separated(
                              scrollDirection: Axis.horizontal,
                              primary: false,
                              padding: const EdgeInsets.symmetric(
                                horizontal: horizontalPadding,
                              ),
                              itemCount: items.length,
                              separatorBuilder: (_, __) =>
                                  const SizedBox(width: spacing),
                              itemBuilder: (_, i) {
                                final m = items[i];
                                final recipe = widget.recipeIndex[m.recipeId];
                                final title = recipe?.title ?? m.title;
                                final imageUrl =
                                    mediaUrl(recipe?.imagePathSmall);
                                return LongPressDraggable<MealPlanEntry>(
                                  data: m,
                                  delay: const Duration(milliseconds: 300),
                                  feedback: Material(
                                    elevation: 6,
                                    borderRadius: BorderRadius.circular(12),
                                    child: SizedBox(
                                      width: tileWidth,
                                      height: tileHeight,
                                      child: RecipeThumbTile(
                                        width: tileWidth,
                                        height: tileHeight,
                                        title: title,
                                        imageUrl: imageUrl,
                                        onOpen: () {},
                                        onDelete: () {},
                                      ),
                                    ),
                                  ),
                                  childWhenDragging: Opacity(
                                    opacity: 0.35,
                                    child: RecipeThumbTile(
                                      width: tileWidth,
                                      height: tileHeight,
                                      title: title,
                                      imageUrl: imageUrl,
                                      onOpen: () {},
                                      onDelete: () {},
                                    ),
                                  ),
                                  child: RecipeThumbTile(
                                    width: tileWidth,
                                    height: tileHeight,
                                    title: title,
                                    imageUrl: imageUrl,
                                    onOpen: () => widget.onOpenRecipe(m.recipeId),
                                    onDelete: () => widget.onUnassign(m),
                                  ),
                                );
                              },
                            ),
                          );
                        },
                      );
                    },
                  ),
                ],
              ),
            ),
          );
        },
      ),
    );
  }
}
