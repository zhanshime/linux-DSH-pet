// 原生 Wayland 下 GTK3 的三件事是否可用：定位 / 点击穿透(input shape) / keep-above
#include <gtk/gtk.h>
#include <cairo.h>
#include <stdio.h>

static gboolean on_map(GtkWidget *w, GdkEvent *e, gpointer d) {
  GdkWindow *gw = gtk_widget_get_window(w);
  printf("GDK window type: %s\n", G_OBJECT_TYPE_NAME(gw));
  GdkRectangle rect = {0, 0, 1, 1};
  cairo_region_t *r = cairo_region_create_rectangle(&rect);
  gdk_window_input_shape_combine_region(gw, r, 0, 0);
  cairo_region_destroy(r);
  printf("input_shape_combine_region 已调用\n");
  gtk_window_move(GTK_WINDOW(w), 1203, 489);
  printf("gtk_window_move 已调用\n");
  gtk_window_set_keep_above(GTK_WINDOW(w), TRUE);
  printf("gtk_window_set_keep_above 已调用\n");
  fflush(stdout);
  return FALSE;
}

int main(int argc, char **argv) {
  gtk_init(&argc, &argv);
  GtkWidget *w = gtk_window_new(GTK_WINDOW_TOPLEVEL);
  gtk_window_set_default_size(GTK_WINDOW(w), 600, 400);
  g_signal_connect(w, "map", G_CALLBACK(on_map), NULL);
  gtk_widget_show_all(w);
  g_timeout_add(1500, (GSourceFunc)gtk_main_quit, NULL);
  gtk_main();
  printf("正常退出\n");
  return 0;
}
