-- Active users with order count
SELECT u.id, u.username, u.email, COUNT(o.id) AS order_count
FROM users u
LEFT JOIN orders o ON o.user_id = u.id
WHERE u.is_active = TRUE
GROUP BY u.id, u.username, u.email
ORDER BY order_count DESC;

-- Top products by revenue
SELECT p.id, p.name, SUM(oi.quantity * oi.price) AS revenue
FROM products p
JOIN order_items oi ON oi.product_id = p.id
JOIN orders o ON o.id = oi.order_id
WHERE o.status = 'completed'
GROUP BY p.id, p.name
ORDER BY revenue DESC
LIMIT 10;

-- Recent orders with user info
SELECT o.id, u.username, o.total, o.status, o.created_at
FROM orders o
JOIN users u ON u.id = o.user_id
WHERE o.created_at > NOW() - INTERVAL '7 days'
ORDER BY o.created_at DESC;

-- Low stock products
SELECT id, name, stock
FROM products
WHERE stock < 10
ORDER BY stock ASC;
