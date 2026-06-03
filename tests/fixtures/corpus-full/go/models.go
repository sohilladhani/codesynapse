package main

import "time"

type User struct {
	ID        int
	Username  string
	Email     string
	CreatedAt time.Time
	IsActive  bool
}

func (u *User) DisplayName() string {
	return u.Username
}

func (u *User) Deactivate() {
	u.IsActive = false
}

type Product struct {
	ID       int
	Name     string
	Price    float64
	Stock    int
	Category string
}

func (p *Product) IsAvailable() bool {
	return p.Stock > 0
}

func (p *Product) ApplyDiscount(pct float64) float64 {
	return p.Price * (1 - pct/100)
}

type Order struct {
	ID     int
	UserID int
	Items  []OrderItem
	Total  float64
	Status string
}

type OrderItem struct {
	ProductID int
	Quantity  int
	Price     float64
}

func (o *Order) AddItem(item OrderItem) {
	o.Items = append(o.Items, item)
	o.Total += item.Price * float64(item.Quantity)
}
