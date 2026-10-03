// The built-in `box-shadow` layer, CSS semantics.
vec4 spread_radius(vec4 radius, float spread) {
    // CSS grows rounded corners with the spread; square corners stay square.
    return vec4(
        radius.x > 0.0 ? max(radius.x + spread, 0.0) : 0.0,
        radius.y > 0.0 ? max(radius.y + spread, 0.0) : 0.0,
        radius.z > 0.0 ? max(radius.z + spread, 0.0) : 0.0,
        radius.w > 0.0 ? max(radius.w + spread, 0.0) : 0.0
    );
}

vec4 paint_main(PaintContext ctx) {
    vec2 p = ctx.frag_phy_px;
    float spread = shoji_shadow_spread;
    if (shoji_shadow_inset < 0.5) {
        vec4 shape = vec4(shoji_shadow_offset - vec2(spread), ctx.size_phy_px + vec2(2.0 * spread));
        float shadow = shoji_blurred_rrect_coverage(
            p, shape, spread_radius(ctx.radius_phy_px, spread), shoji_shadow_sigma);
        // An outer shadow is never painted below the box itself.
        return shoji_shadow_color * shadow * (1.0 - shoji_outer_coverage(ctx));
    }

    vec4 inner = ctx.inner_rect_phy_px;
    vec4 lit_shape = vec4(
        inner.xy + shoji_shadow_offset + vec2(spread),
        inner.zw - vec2(2.0 * spread)
    );
    float lit = shoji_blurred_rrect_coverage(
        p, lit_shape, spread_radius(ctx.inner_radius_phy_px, -spread), shoji_shadow_sigma);
    return shoji_shadow_color * (1.0 - lit) * shoji_inner_coverage(ctx);
}
