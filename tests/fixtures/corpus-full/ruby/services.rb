require_relative 'models'

class Database
  def initialize
    @store = {}
  end

  def save_user(user)
    @store["user:#{user.id}"] = user
  end

  def find_user(id)
    @store["user:#{id}"]
  end

  def save_product(product)
    @store["product:#{product.id}"] = product
  end

  def find_product(id)
    @store["product:#{id}"]
  end
end

class UserService
  def initialize(db)
    @db = db
  end

  def register(username, email)
    user = User.new(rand(1_000_000), username, email)
    @db.save_user(user)
    user
  end

  def find_by_id(id)
    @db.find_user(id)
  end

  def deactivate(id)
    user = @db.find_user(id)
    return false unless user
    user.deactivate
    @db.save_user(user)
    true
  end
end

class ProductService
  def initialize(db)
    @db = db
  end

  def get_by_id(id)
    @db.find_product(id)
  end

  def restock(id, qty)
    product = @db.find_product(id)
    return false unless product
    product.stock += qty
    @db.save_product(product)
    true
  end
end
