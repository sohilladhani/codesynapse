require 'time'

class User
  attr_reader :id, :username, :email, :created_at
  attr_accessor :is_active

  def initialize(id, username, email)
    @id = id
    @username = username
    @email = email
    @created_at = Time.now
    @is_active = true
  end

  def display_name
    @username
  end

  def deactivate
    @is_active = false
  end
end

class Product
  attr_reader :id, :name, :price, :category
  attr_accessor :stock

  def initialize(id, name, price, stock: 0, category: nil)
    @id = id
    @name = name
    @price = price
    @stock = stock
    @category = category
  end

  def available?
    @stock > 0
  end

  def apply_discount(pct)
    @price * (1 - pct / 100.0)
  end
end

class Order
  attr_reader :id, :user_id, :items, :status
  attr_accessor :total

  def initialize(id, user_id)
    @id = id
    @user_id = user_id
    @items = []
    @total = 0.0
    @status = 'pending'
  end

  def add_item(product, qty = 1)
    @items << { product_id: product.id, quantity: qty, price: product.price }
    @total += product.price * qty
  end
end
