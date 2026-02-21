import 'package:flutter/material.dart';
import '../../api.dart';
import 'recipe_thumb_tile.dart';

class DayTimelineBox extends StatelessWidget {
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
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    final cardShape = RoundedRectangleBorder(
      side: isToday
          ? BorderSide(color: theme.colorScheme.primary, width: 2)
          : BorderSide.none,
      borderRadius: BorderRadius.circular(14),
    );

    final cardColor = isToday
        ? theme.colorScheme.primaryContainer.withValues(alpha: 0.08)
        : null;

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      child: Card(
        elevation: isToday ? 2.5 : 1.5,
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
                    dayLabel,
                    style: theme.textTheme.titleMedium?.copyWith(
                      fontWeight: isToday ? FontWeight.w600 : null,
                    ),
                  ),
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
                          child: CircularProgressIndicator(strokeWidth: 2),
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

                  // Make tiles ~10% smaller so two fit across with spacing.
                  const spacing = 10.0;
                  const horizontalPadding = 8.0;

                  return LayoutBuilder(
                    builder: (context, constraints) {
                      final maxW = constraints.maxWidth;
                      // Two tiles + one gap should fit; then shrink ~10%.
                      final base = (maxW - horizontalPadding * 2 - spacing) / 2;
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
                            final recipe = recipeIndex[m.recipeId];
                            final title = recipe?.title ?? m.title;
                            final imageUrl = mediaUrl(recipe?.imagePathSmall, cacheBuster: recipe?.updatedAt);
                            return RecipeThumbTile(
                              width: tileWidth,
                              height: tileHeight,
                              title: title,
                              imageUrl: imageUrl,
                              onOpen: () => onOpenRecipe(m.recipeId),
                              onDelete: () => onUnassign(m),
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
      ),
    );
  }
}
